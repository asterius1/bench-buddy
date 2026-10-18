"""A recorded live feed judged as the live judge would, next to the log of the same pull.

    python tools/livereplay.py [SESSION_DIR]

SESSION_DIR is a feed recording of the addon (`<GW2>/addons/bench_buddy/sessions/<stamp>`, default
the newest, see feedcheck.py). For every golem log written while it recorded, the feed's events of
that pull go through core/live_server.py's compare_live() and the log through the checker, and the
two are printed side by side: per cycle (Scrapper) or row (Berserker) its length and the casts
matched, missed and extra. Live can only be as good as the casts it can put back from hits.
"""
import json
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "core"))
sys.path.insert(0, str(ROOT / "tools"))
import evtc_rot  # noqa: E402
import feedcheck  # noqa: E402
import live_server  # noqa: E402

ELITES = {18: "berserker", 43: "scrapper"}


def feed_events(path):
    """The area feed's events of yours, in the shape the addon sends. Old recordings have no
    "dst_self": your agent is the target of your initial buffs (state change 18)."""
    evs = []
    for line in open(path, encoding="utf8"):
        try:
            j = json.loads(line)
        except ValueError:
            continue
        e = j.get("ev")
        if j.get("feed") == "area" and e and (j.get("src") or {}).get("self"):
            evs.append(e)
    me = Counter(e["dst_agent"] for e in evs if e["is_statechange"] == 18).most_common(1)
    me = me[0][0] if me else None
    out = [{"time": e["time"], "sc": e["is_statechange"], "buff": e["buff"], "skill": e["skill_id"],
            "value": e["value"], "buff_dmg": e["buff_dmg"], "result": e["result"],
            "activation": e["is_activation"], "buffremove": e["is_buffremove"], "dst": e["dst_agent"],
            "dst_self": e.get("dst_self", e["dst_agent"] == me)} for e in evs]
    return sorted(out, key=lambda e: e["time"])


def chips(rows, minis):
    c = Counter()
    for r in rows:
        for m in minis(r):
            for s in (m or {}).get("sections", {}).values():
                c.update(x["kind"] for x in s["chips"])
    return c


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    sess = Path(sys.argv[1]) if len(sys.argv) > 1 else feedcheck.newest_session()
    evs = feed_events(sess / "combat.jsonl")
    if not evs:
        print("no feed events in", sess)
        return
    lo, hi = evs[0]["time"], evs[-1]["time"]
    for p in sorted(evtc_rot.GOLEM.glob("*.zevtc")):
        if p.stem < evtc_rot.FIRST_ANIMATED:
            continue
        agents, _, events = evtc_rot.evtc.load(p)
        a, b = events[0].time, events[-1].time
        if not (lo <= a <= hi):
            continue
        elite = evtc_rot.read(p)["elite"]
        kind = ELITES.get(elite)
        if not kind:
            continue
        # the pull's events: from shortly before its first hit (casts under way) to its end
        pe = [e for e in evs if a - 3000 <= e["time"] <= b]
        got = live_server.compare_live({"elite": elite, "events": pe})["data"]
        if kind == "scrapper":
            # live returns the panel (scrapper/panel.py), so the log's panel is what to hold it against
            log = live_server.serve.scrap_panel.panel(p)
            row = lambda g: (g["n"], round(g["len"] / 1000, 2) if g["len"] else None, g["hits"]["net"])  # noqa: E731
            print(f"\n{p.stem} Scrapper: log {len(log['rows'])} cycles, live {len(got['rows']) if got else 0}"
                  "   (cycle, length s, hits against the bench)")
            print("  log ", [row(g) for g in log["rows"]])
            print("  live", [row(g) for g in (got or {}).get("rows", [])])
            print("  behind log", log["head"][0][1], " live", got["head"][0][1] if got else None)
        else:
            log = live_server.serve.local.compare(p)
            parts = lambda r: [r[k] for k in ("opener", "sword", "bow") if k in r]  # noqa: E731
            print(f"\n{p.stem} Berserker: log {len(log['rows'])} rows, live {len(got['rows']) if got else 0}")
            print("  log chips: ", dict(chips(log["rows"], parts)))
            print("  live chips:", dict(chips((got or {}).get("rows", []), parts)))
            print("  lag log ", log["lag"][:8])
            print("  lag live", (got or {}).get("lag", [])[:8])


if __name__ == "__main__":
    main()
