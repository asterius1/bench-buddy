"""Casts from hits alone, the way the in-game addon has to do it live.

    python tools/livecast.py [SESSION_DIR]

The live arcdps feed carries your hits but no casts (feedcheck.py). Every damaging skill's first
hit lands a fixed time after its cast starts, so a cast can be put back from its first hit.

1. From every golem log of every build (raw, core/evtc_rot's reader, read-only) it measures per skill
   the time from cast start to first hit (median), from first to last hit of one cast (95th
   percentile, the window inside which further hits belong to the same cast) and your cast length
   (median), and writes them to `core/livecasts.json`, which the live judge (core/live.py) reads.
   Skills that do no damage are put back from a buff they put on you at a fixed time after the cast
   starts (Flames of War, Blood Reckoning; found by `signatures()` for any build, two buffs together
   when one alone also comes from elsewhere, as Superspeed does).
2. On a recording of the live feed it puts the casts back as the live judge does (core/live.py) and
   checks each against the logs of the same minutes: found (a real cast of that skill within 60 ms),
   wrong (none) and the real casts it never guessed.
"""
import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import feedcheck  # noqa: E402
import livereplay  # noqa: E402
import live  # noqa: E402  (core/live.py)

R = feedcheck.evtc_rot
TABLE = HERE.parent / "core" / "livecasts.json"
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


def all_logs():
    """Every golem log with casts, of any build."""
    return [p for p in sorted(R.GOLEM.glob("*.zevtc")) if p.stem >= R.FIRST_ANIMATED]


def measure():
    off, span, gaps, names = defaultdict(list), defaultdict(list), defaultdict(list), {}
    boff, durs = defaultdict(list), defaultdict(list)
    sig_starts, sig_applies, sig_hit = defaultdict(list), defaultdict(list), Counter()
    for n_log, p in enumerate(all_logs()):
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
            if e.src == pov and e.sc == 68 and e.value > 0:
                durs[e.skill].append(e.value)
        for e in events:
            if e.sc == 69 and e.dst == pov:
                sig_applies[e.skill].append((n_log, e.time))
        for sid, ts in starts.items():
            for i, t in enumerate(ts):
                nxt = ts[i + 1] if i + 1 < len(ts) else float("inf")
                hs = [h for h in hits.get(sid, []) if t <= h < nxt]
                sig_starts[(sid, agents[pov].elite)].append((n_log, t))
                sig_hit[(sid, agents[pov].elite)] += bool(hs)
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
                           "window": s[len(s) * 19 // 20] + 150, "pulse": pulse, "casts": len(o),
                           "dur": round(statistics.median(durs[sid])) if durs[sid] else None}
    for sid, o in boff.items():
        o = sorted(o)
        table[str(sid)] = {"name": names[sid], "buff": BUFF[sid], "offset": round(statistics.median(o)),
                           "spread": o[len(o) * 19 // 20] - o[len(o) // 20], "window": 0, "pulse": 0,
                           "casts": len(o), "dur": round(statistics.median(durs[sid])) if durs[sid] else None}
    # a skill several builds cast (dodge) gets one rule per build: "skill/elite"
    sigs = signatures(sig_starts, sig_applies, sig_hit)
    for (sid, elite), rule in sigs.items():
        shared = len({e for s_, e in sig_starts if s_ == sid}) > 1
        key = f"{sid}/{elite}" if shared else str(sid)
        if key not in table:
            table[key] = {"name": names[sid], **rule, "window": 0, "pulse": 0,
                          "dur": round(statistics.median(durs[sid])) if durs[sid] else None}
    return table


SIG_PRESENT = 0.9     # the buff comes after this share of the skill's casts
SIG_PAIRED = 0.6      # the second buff of a pair, after this share
SIG_PRECISE = 0.8     # and this share of the buff's applies (or the pair's) are such casts
SIG_SPREAD = 150      # ms, 5th to 95th percentile of the time from cast start to the buff
SIG_AFTER = 1500      # ms after the cast start to look for it


def signatures(starts, applies, hit):
    """{(skill, elite): {"buff", "offset", "spread", "casts", ["with", "gap"]}} for skills that rarely
    hit: a buff on you at a steady time after the cast starts, rare otherwise; or two together, the
    second one then needing to come after only SIG_PAIRED of the casts."""
    out = {}
    by_log = defaultdict(lambda: defaultdict(list))
    for b, ts in applies.items():
        for n, t in ts:
            by_log[b][n].append(t)
    for sid, ss in starts.items():
        if len(ss) < MIN_CASTS or hit[sid] > 0.5 * len(ss):
            continue
        cands = []
        for b in applies:
            offs = [min((x - t for x in by_log[b][n] if 0 <= x - t <= SIG_AFTER), default=None) for n, t in ss]
            offs = sorted(o for o in offs if o is not None)
            if len(offs) < SIG_PAIRED * len(ss):
                continue
            spread = offs[len(offs) * 19 // 20] - offs[len(offs) // 20]
            if spread > SIG_SPREAD:
                continue
            off, tol = round(statistics.median(offs)), max(60, spread)
            at = {(n, t + off) for n, t in ss}
            mine = [(n, x) for n, x in applies[b] if any(abs(x - y) <= tol for m, y in at if m == n)]
            cands.append((len(mine) / len(applies[b]), b, off, spread, len(offs) >= SIG_PRESENT * len(ss)))
        cands.sort(reverse=True)
        single = [c for c in cands if c[4]]
        if single and single[0][0] >= SIG_PRECISE:
            cands = single
            prec, b, off, spread, _ = cands[0]
            out[sid] = {"buff": b, "offset": off, "spread": spread, "casts": len(ss)}
            continue
        # two buffs: the first with the second `gap` ms later, which neither alone tells
        best = None
        for p1, b1, o1, s1, often in cands:
            for p2, b2, o2, s2, _ in cands:
                if b1 == b2 or o2 < o1 or not often:
                    continue
                gap = o2 - o1
                pair = [(n, x) for n, x in applies[b1] if any(abs(y - x - gap) <= 60 for y in by_log[b2][n])]
                if not pair:
                    continue
                at = {(n, t + o1) for n, t in ss}
                good = sum(1 for n, x in pair if any(abs(x - y) <= max(60, s1) for m, y in at if m == n))
                prec = good / len(pair)
                if prec >= SIG_PRECISE and (best is None or prec > best[0]):
                    best = (prec, {"buff": b1, "with": b2, "gap": gap, "offset": o1, "spread": s1, "casts": len(ss)})
        if best:
            out[sid] = best[1]
    return out


def main():
    table = measure()
    TABLE.parent.mkdir(exist_ok=True)
    TABLE.write_text(json.dumps(table, indent=1) + "\n", encoding="utf8")
    print(f"{len(table)} skills -> {TABLE.relative_to(HERE.parent)}")
    sess = Path(sys.argv[1]) if len(sys.argv) > 1 else feedcheck.newest_session()
    events = livereplay.feed_events(sess / "combat.jsonl")
    if not events:
        print("no feed events in", sess)
        return
    lo, hi = events[0]["time"], events[-1]["time"]
    # each pull's casts put back as the live judge does (core/live.py), against its log
    guessed, real = [], []
    for p in all_logs():
        agents, skills, evs = R.evtc.load(p)
        pov = next((e.src for e in evs if e.sc == 13), None)
        a, b = evs[0].time, evs[-1].time
        if not (lo <= a <= hi):
            continue
        real += [(e.time, e.skill) for e in evs if e.src == pov and e.sc == 67]
        mine = [e for e in events if a - 3000 <= e["time"] <= b]
        guessed += [(c["t"], c["id"]) for c in live.casts_from(mine, live.table(agents[pov].elite)) if a <= c["t"] <= b]
    real_by = defaultdict(list)
    for t, s in real:
        real_by[s].append(t)
    ok, wrong, err = Counter(), Counter(), []
    hit_real = set()
    for t, s in guessed:
        best = min(real_by.get(s, []), key=lambda r: abs(r - t), default=None)
        rule = table.get(str(s)) or next((v for k, v in table.items() if k.startswith(f"{s}/")), {})
        if best is not None and abs(best - t) <= max(MATCH_MS, rule.get("spread", 0) + 30):
            ok[s] += 1
            err.append(t - best)
            hit_real.add((best, s))
        else:
            wrong[s] += 1
    missed = Counter(s for t, s in real if (t, s) not in hit_real)
    name = lambda s: next((v["name"] for k, v in table.items() if k.split("/")[0] == str(s)), str(s))  # noqa: E731
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
