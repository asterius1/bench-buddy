"""Casts from hits alone, the way the in-game addon has to do it live.

    python nexus/livecast.py [SESSION_DIR]

The live arcdps feed carries your hits but no casts (feedcheck.py). Every damaging skill's first
hit lands a fixed time after its cast starts, so a cast can be put back from its first hit.

1. From every Berserker golem log (raw, rotation/evtc_rot's reader, read-only) it measures per skill
   the time from cast start to first hit (median) and from first to last hit of one cast (95th
   percentile, the window inside which further hits belong to the same cast), and writes them to
   `nexus/addon/casts.json`, which the addon compiles in.
   Flames of War and Blood Reckoning do no damage: they are put back from the buff they apply.
2. On a recording of the live feed it applies the rule to the area feed's hits and checks
   each guessed cast against the logs of the same minutes: found (a real cast of that skill within
   60 ms), wrong (none) and the real casts it never guessed.
"""
import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import feedcheck  # noqa: E402

R = feedcheck.evtc_rot
TABLE = HERE.parent / "addon" / "casts.json"
MATCH_MS = 60
MIN_CASTS = 5
# skills that do no damage of their own show live by the buff they put on you (baseline.BUFF)
BUFF = {29940: 31708, 30189: 29466}
PULSE_TOL = 60


def my_hits(events, pov):
    hits = defaultdict(list)
    for e in events:
        if e.src == pov and e.sc == 0 and e.buff == 0 and e.dst != 0 and not e.buffremove and not e.activation:
            hits[e.skill].append(e.time)
    return hits


def measure():
    off, span, gaps, names = defaultdict(list), defaultdict(list), defaultdict(list), {}
    boff = defaultdict(list)
    for p in R.golem_logs():
        agents, skills, events = R.evtc.load(p)
        pov = next((e.src for e in events if e.sc == 13), None)
        hits = my_hits(events, pov)
        applies = defaultdict(list)
        for e in events:
            if e.sc == 69 and e.dst == pov and e.src == pov and e.skill in BUFF.values():
                applies[e.skill].append(e.time)
        starts = defaultdict(list)
        for e in events:
            if e.src == pov and e.sc == 67:
                starts[e.skill].append(e.time)
                names[e.skill] = skills.get(e.skill, str(e.skill))
        for sid, ts in starts.items():
            for i, t in enumerate(ts):
                nxt = ts[i + 1] if i + 1 < len(ts) else float("inf")
                hs = [h for h in hits.get(sid, []) if t <= h < nxt]
                if sid in BUFF:
                    a = [x for x in applies[BUFF[sid]] if 0 <= x - t < 2000 and x < nxt]
                    if a:
                        boff[sid].append(a[0] - t)
                if hs:
                    off[sid].append(hs[0] - t)
                    span[sid].append(hs[-1] - hs[0])
                    gaps[sid] += [h - hs[0] for h in hs[1:]]
    table = {}
    for sid, o in off.items():
        if len(o) < MIN_CASTS:
            continue
        s, o = sorted(span[sid]), sorted(o)
        # a field that pulses (Scorched Earth every 2 s): later hits of one cast come at whole
        # multiples of the pulse after its first, so a hit off that beat is a new cast
        g = gaps[sid]
        pulse = Counter(round(x / 50) * 50 for x in g).most_common(1)[0][0] if len(g) >= MIN_CASTS else 0
        on_beat = sum(abs((x + PULSE_TOL) % pulse - PULSE_TOL) <= PULSE_TOL for x in g) if pulse >= 500 else 0
        if on_beat < 0.6 * len(g):
            pulse = 0
        table[str(sid)] = {"name": names[sid], "offset": round(statistics.median(o)),
                           "spread": o[len(o) * 19 // 20] - o[len(o) // 20],
                           "window": s[len(s) * 19 // 20] + 150, "pulse": pulse, "casts": len(o)}
    for sid, o in boff.items():
        o = sorted(o)
        table[str(sid)] = {"name": names[sid], "buff": BUFF[sid], "offset": round(statistics.median(o)),
                           "spread": o[len(o) * 19 // 20] - o[len(o) // 20], "window": 0, "pulse": 0,
                           "casts": len(o)}
    return table


def guess(hits, table, applies=None):
    """[(start, skill)] from {skill: [hit times]} and {buff: [apply times]}."""
    out = []
    for sid, t in table.items():
        if "buff" in t:
            out += [(a - t["offset"], int(sid)) for a in (applies or {}).get(t["buff"], [])]
    for sid, hs in hits.items():
        t = table.get(str(sid))
        if not t or "buff" in t:
            continue
        open_ = []   # first hits of this skill's casts that can still be hitting
        for h in sorted(hs):
            open_ = [f for f in open_ if h - f <= t["window"]]
            p = t["pulse"]
            if any(not p or abs((h - f + PULSE_TOL) % p - PULSE_TOL) <= PULSE_TOL for f in open_):
                continue
            open_.append(h)
            out.append((h - t["offset"], sid))
    return sorted(out)


def main():
    table = measure()
    TABLE.parent.mkdir(exist_ok=True)
    TABLE.write_text(json.dumps(table, indent=1) + "\n", encoding="utf8")
    print(f"{len(table)} skills -> {TABLE.relative_to(HERE.parent)}")
    sess = Path(sys.argv[1]) if len(sys.argv) > 1 else feedcheck.newest_session()
    feeds = feedcheck.load_feed(sess / "combat.jsonl")
    area = feeds.get("area", {})
    if not area:
        print("no area feed events in", sess)
        return
    lo, hi = min(k[0] for k in area), max(k[0] for k in area)
    live, applies = defaultdict(list), defaultdict(list)
    for k in area:
        if k[1] == "direct hit":
            live[k[2]].append(k[0])
        if k[1] == "buff apply":
            applies[k[2]].append(k[0])
    guessed = guess(live, table, applies)
    real, spans = [], []
    for p in R.golem_logs():
        agents, skills, events = R.evtc.load(p)
        pov = next((e.src for e in events if e.sc == 13), None)
        if not (lo <= events[0].time <= hi):
            continue
        real += [(e.time, e.skill) for e in events if e.src == pov and e.sc == 67]
        spans.append((events[0].time, events[-1].time))
    # hits between pulls (precasts on the golem before arcdps starts the log) have no log to check
    guessed = [(t, s) for t, s in guessed if any(a <= t <= b for a, b in spans)]
    real_by = defaultdict(list)
    for t, s in real:
        real_by[s].append(t)
    ok, wrong, err = Counter(), Counter(), []
    hit_real = set()
    for t, s in guessed:
        best = min(real_by.get(s, []), key=lambda r: abs(r - t), default=None)
        if best is not None and abs(best - t) <= max(MATCH_MS, table[str(s)]["spread"] + 30):
            ok[s] += 1
            err.append(t - best)
            hit_real.add((best, s))
        else:
            wrong[s] += 1
    missed = Counter(s for t, s in real if (t, s) not in hit_real)
    name = lambda s: table.get(str(s), {}).get("name") or str(s)  # noqa: E731
    print(f"\nsession {sess.name}: {len(real)} real casts, {len(guessed)} guessed from live hits")
    print(f"found {sum(ok.values())}, wrong {sum(wrong.values())}, never guessed {sum(missed.values())}")
    if err:
        e = sorted(abs(x) for x in err)
        print(f"start error median {statistics.median(e):.0f} ms, 95% {e[len(e) * 19 // 20]} ms")
    print(f"\n{'skill':<22}{'real':>6}{'found':>7}{'wrong':>7}{'missed':>8}")
    for s in sorted(real_by, key=lambda s: -len(real_by[s])):
        print(f"{name(s)[:22]:<22}{len(real_by[s]):>6}{ok[s]:>7}{wrong[s]:>7}{missed[s]:>8}")


if __name__ == "__main__":
    main()
