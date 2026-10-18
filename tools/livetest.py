"""The live path against the log, for every pull that has both.

    python tools/livetest.py                  every pull with a capture: how live and the log differ
    python tools/livetest.py STEM...          only these pulls (log stems, e.g. 20261018-022308)
    python tools/livetest.py --import [DIR]   cut captures out of the addon's feed recordings first
    python tools/livetest.py --save           take this run as the baseline the next runs are held to

A capture is `<log>.live.json` next to the arcdps log (core/live.py `capture_of`): the events the
addon last sent the live judge for that pull, saved when the log came in with "Save live captures"
on in the addon's options. `--import` makes them for older pulls from the recordings in
`<GW2>/addons/bench_buddy/sessions/` (area feed only, so hit times are exact but nothing is local).

For each pull, two things are held side by side:
  casts   the record core/live.py puts back from the feed against evtc_rot.read() of the log. A log
          cast is "found" when live has a cast of the same skill within 300 ms, "missed" when not,
          "unseen" when live can't see that skill at all (not in core/livecasts.json: heals, kit
          swaps). Live casts with no log cast are "extra". Zero-length log casts are left out.
  judged  the build's module on the live record (as core/live_server.py runs it) against the
          same module on the log: Scrapper cycles (length, hits against the bench), Berserker chips.

With a baseline (`--save`, kept next to the logs as livetest_baseline.json) a pull that finds fewer
casts or more extra ones than before is marked WORSE and the run exits with 1.
"""
import json
import statistics
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "core"))
sys.path.insert(0, str(ROOT / "tools"))
import evtc_rot  # noqa: E402
import live  # noqa: E402
import live_server  # noqa: E402

MATCH_MS = 300
LEN_TOL_S = 0.05   # cycle lengths this close count as the same (live times are exact to ~2 ms)
BASELINE = evtc_rot.GOLEM / "livetest_baseline.json"


def import_sessions(dirs):
    """Writes a capture for every golem log a feed recording covers and that has none yet."""
    import feedcheck
    import livereplay
    if not dirs:
        dirs = [d for root in feedcheck.SESSION_DIRS if root.is_dir() for d in sorted(root.iterdir())]
    logs = [p for p in sorted(evtc_rot.GOLEM.glob("*.zevtc")) if p.stem >= evtc_rot.FIRST_ANIMATED]
    made = 0
    for d in dirs:
        f = Path(d) / "combat.jsonl"
        if not f.is_file() or not f.stat().st_size:
            continue
        evs = livereplay.feed_events(f)
        if not evs:
            continue
        lo, hi = evs[0]["time"], evs[-1]["time"]
        for p in logs:
            out = live.capture_of(p)
            if out.exists():
                continue
            try:
                _, _, events = evtc_rot.evtc.load(p)
            except Exception:  # noqa: BLE001
                continue
            a, b = events[0].time, events[-1].time
            if not (lo <= a <= hi):
                continue
            pe = [e for e in evs if a - 3000 <= e["time"] <= b]
            out.write_text(json.dumps({"log": p.stem, "elite": evtc_rot.read(p)["elite"], "events": pe,
                                       "source": f"session {Path(d).name}"}), encoding="utf8")
            made += 1
    print(f"imported {made} captures from {len(dirs)} recordings")


def match_casts(log, got, seen):
    """found [(log cast, live cast)], missed, unseen, extra."""
    log = [c for c in log if c["dur"] > 0]
    free = sorted(got, key=lambda c: c["t"])
    pairs = sorted(((abs(a["t"] - b["t"]), i, j) for i, a in enumerate(log) for j, b in enumerate(free)
                    if a["id"] == b["id"] and abs(a["t"] - b["t"]) <= MATCH_MS))
    li, lj, found = set(), set(), []
    for _, i, j in pairs:
        if i not in li and j not in lj:
            li.add(i)
            lj.add(j)
            found.append((log[i], free[j]))
    rest = [c for i, c in enumerate(log) if i not in li]
    missed = [c for c in rest if c["id"] in seen]
    unseen = [c for c in rest if c["id"] not in seen]
    extra = [c for j, c in enumerate(free) if j not in lj]
    return found, missed, unseen, extra


def judged(kind, data):
    """A short, comparable summary of what the module made of a pull."""
    if not data:
        return None
    if kind == "scrapper":
        return [(g["n"], round(g["len"] / 1000, 2) if g["len"] else None, g["hits"]["net"]) for g in data["rows"]]
    if kind == "berserker":
        c = Counter()
        for r in data["rows"]:
            for k in ("opener", "sword", "bow"):
                for s in (r.get(k) or {}).get("sections", {}).values():
                    c.update(x["kind"] for x in s["chips"])
        return dict(sorted(c.items()))
    return sorted(data) if isinstance(data, dict) else data


def same(kind, a, b):
    if kind == "scrapper" and a and b and len(a) == len(b):
        return all(x[0] == y[0] and x[2] == y[2] and (x[1] == y[1] or None not in (x[1], y[1])
                   and abs(x[1] - y[1]) <= LEN_TOL_S) for x, y in zip(a, b))
    return a == b


def names(cs):
    return ", ".join(f"{n} x{k}" for n, k in Counter(c["name"] for c in cs).most_common(4))


def one(path):
    cap = json.loads(path.read_text(encoding="utf8"))
    log_path = path.with_name(path.name[: -len(".live.json")] + ".zevtc")
    elite = cap.get("elite")
    j = live_server.judge_for(elite)
    if j is None:
        return {"log": log_path.stem, "skip": f"no module for elite {elite}"}
    kind, mod, fn = j
    want = evtc_rot.read(log_path)
    rec = live.record(cap["events"], elite)
    if rec and hasattr(mod, "live"):
        rec = mod.live(rec)
    found, missed, unseen, extra = match_casts(want["casts"], (rec or {}).get("casts", []), set(live.table(elite)))
    dt = [b["t"] - a["t"] for a, b in found]
    got = live_server.compare_live({"elite": elite, "events": cap["events"]})["data"]
    a, b = judged(kind, fn(log_path)), judged(kind, got)
    return {"log": log_path.stem, "kind": kind, "source": cap.get("source", ""),
            "found": len(found), "missed": len(missed), "unseen": len(unseen), "extra": len(extra),
            "dt": round(statistics.median(dt)) if dt else None,
            "dt_spread": (min(dt), max(dt)) if dt else None,
            "swaps": (len(want["swaps"]), len((rec or {}).get("swaps", []))) if hasattr(mod, "live") else None,
            "missed_names": names(missed), "extra_names": names(extra), "unseen_names": names(unseen),
            "by_skill": {k: Counter(c["name"] for c in v) for k, v in
                         (("missed", missed), ("extra", extra), ("unseen", unseen))},
            "judged_log": a, "judged_live": b, "same": same(kind, a, b)}


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    args = sys.argv[1:]
    if "--import" in args:
        i = args.index("--import")
        dirs = [a for a in args[i + 1:] if not a.startswith("--")]
        import_sessions(dirs)
        args = [a for a in args[:i] if a != "--import"]
    save = "--save" in args
    stems = [a for a in args if not a.startswith("--")]
    caps = sorted(evtc_rot.GOLEM.glob("*.live.json"))
    if stems:
        caps = [c for c in caps if any(c.name.startswith(s) for s in stems)]
    caps = [c for c in caps if c.with_name(c.name[: -len(".live.json")] + ".zevtc").is_file()]
    if not caps:
        print("no captures next to the logs in", evtc_rot.GOLEM)
        print('turn on "Save live captures" in the addon options, or run with --import')
        return 0
    base = json.loads(BASELINE.read_text(encoding="utf8")) if BASELINE.is_file() and not save else {}
    tot, worse, out = Counter(), [], {}
    skills = {k: Counter() for k in ("missed", "extra", "unseen")}
    for c in caps:
        try:
            r = one(c)
        except Exception as e:  # noqa: BLE001
            print(f"\n{c.name}: FAILED {type(e).__name__}: {e}")
            worse.append(c.name)
            continue
        if "skip" in r:
            print(f"\n{r['log']}: skipped, {r['skip']}")
            continue
        out[r["log"]] = {k: r[k] for k in ("found", "missed", "extra")}
        tot.update({k: r[k] for k in ("found", "missed", "unseen", "extra")})
        tot["pulls"] += 1
        tot["same"] += r["same"]
        for k, v in r["by_skill"].items():
            skills[k].update(v)
        b = base.get(r["log"])
        flag = ""
        if b and (r["found"] < b["found"] or r["extra"] > b["extra"]):
            flag = f"   WORSE than baseline (found {b['found']}, extra {b['extra']})"
            worse.append(r["log"])
        print(f"\n{r['log']} {r['kind']} ({r['source']}){flag}")
        n = r["found"] + r["missed"] + r["unseen"]
        print(f"  casts  found {r['found']}/{n}, missed {r['missed']}, unseen {r['unseen']}, extra {r['extra']};"
              f" start off by {r['dt']} ms median, {r['dt_spread']} range")
        for k in ("missed", "extra", "unseen"):
            if r[f"{k}_names"]:
                print(f"    {k}: {r[k + '_names']}")
        if r["swaps"]:
            print(f"  swaps  log {r['swaps'][0]}, live {r['swaps'][1]}")
        print(f"  judged {'the same' if r['same'] else 'DIFFERENT'}")
        if not r["same"]:
            print(f"    log  {r['judged_log']}")
            print(f"    live {r['judged_live']}")
    n = tot["found"] + tot["missed"] + tot["unseen"]
    print(f"\n{tot['pulls']} pulls: casts found {tot['found']}/{n}, missed {tot['missed']}, unseen {tot['unseen']},"
          f" extra {tot['extra']}; judged the same on {tot['same']}/{tot['pulls']}")
    for k, v in skills.items():
        if v:
            print(f"  {k} most: " + ", ".join(f"{n} x{c}" for n, c in v.most_common(6)))
    if save:
        BASELINE.write_text(json.dumps(out, indent=1), encoding="utf8")
        print("baseline saved:", BASELINE)
    elif worse:
        print("worse than the baseline or failed:", ", ".join(worse))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
