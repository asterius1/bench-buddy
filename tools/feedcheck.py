"""How much of a golem pull the live arcdps feeds carry, against the log of the same pull.

    python nexus/feedcheck.py [SESSION_DIR]

SESSION_DIR is a feed recording (`<GW2>/addons/bench_buddy/sessions/<stamp>` from the addon;
default the newest)
whose `combat.jsonl` holds every event of both feeds Nexus forwards ("area" = squad, "local") with
its arrival time `t` (microseconds since the session start). Each Berserker golem log written while
it recorded is read raw (rotation/evtc_rot's reader, read-only) and every event of the player is
looked up in each feed by (time, kind, skill, value). Arrival delay is the event's arrival minus its
own time, minus the smallest such difference in the session (so 0 = the fastest event seen).
"""
import json
import os
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "core"))
import evtc_rot  # noqa: E402

# the game's addons folder; set BENCH_BUDDY_ADDONS when the game is installed elsewhere
ADDONS = Path(os.environ.get("BENCH_BUDDY_ADDONS") or r"C:\Program Files\Guild Wars 2\addons")
# the addon's recordings of the feed
SESSION_DIRS = [ADDONS / "bench_buddy" / "sessions"]


def newest_session():
    """The newest recording that holds a feed, by its file's time."""
    found = [d / "combat.jsonl" for root in SESSION_DIRS if root.is_dir() for d in root.iterdir()
             if (d / "combat.jsonl").is_file() and (d / "combat.jsonl").stat().st_size > 0]
    return max(found, key=lambda f: f.stat().st_mtime).parent


def kind(sc, buff, buffremove, activation):
    if sc == 0:
        if activation:
            return "activation"
        if buffremove:
            return "buff remove"
        return "condition tick" if buff else "direct hit"
    return {67: "cast start", 68: "cast end", 11: "weapon swap", 69: "buff apply", 70: "buff extend",
            71: "buff active", 72: "buff gone"}.get(sc, f"state change {sc}")


def load_feed(path):
    """{feed: {key: arrival_us}}, key = (time, kind, skill, value); first arrival of duplicates."""
    feeds = defaultdict(dict)
    for line in open(path, encoding="utf8"):
        try:
            j = json.loads(line)
        except ValueError:
            continue
        e = j.get("ev")
        if not e or not (j.get("src") or {}).get("self"):
            continue
        k = (e["time"], kind(e["is_statechange"], e["buff"], e["is_buffremove"], e["is_activation"]),
             e["skill_id"], e["value"])
        feeds[j["feed"]].setdefault(k, j["t"])
    return feeds


def log_events(path):
    agents, skills, events = evtc_rot.evtc.load(path)
    pov = next((e.src for e in events if e.sc == 13), None)
    me = agents.get(pov) or next(a for a in agents.values() if a.is_player)
    out = []
    for e in events:
        if e.src != me.addr:
            continue
        k = kind(e.sc, e.buff, getattr(e, "buffremove", 0), getattr(e, "activation", 0))
        if e.sc == 0 and e.dst == 0:
            continue
        out.append(((e.time, k, e.skill, e.value), skills.get(e.skill, str(e.skill))))
    return me, out


def main():
    sess = Path(sys.argv[1]) if len(sys.argv) > 1 else newest_session()
    feeds = load_feed(sess / "combat.jsonl")
    allk = [k for f in feeds.values() for k in f]
    if not allk:
        print("no events of yours in", sess)
        return
    lo, hi = min(k[0] for k in allk), max(k[0] for k in allk)
    base = min(t - k[0] * 1000 for f in feeds.values() for k, t in f.items())
    found = defaultdict(Counter)    # feed -> kind -> found
    total = Counter()
    delay = defaultdict(list)       # feed -> delays in s, direct hits
    casts_hit = [0, 0]              # casts with a direct hit of the skill in the area feed by cast end + 1 s
    first_gap = []                  # first such hit after the cast start, s
    logs = 0
    for p in evtc_rot.golem_logs():
        me, evs = log_events(p)
        if not evs or not (lo <= evs[0][0][0] <= hi):
            continue
        logs += 1
        for k, _ in evs:
            total[k[1]] += 1
            for f, m in feeds.items():
                if k in m:
                    found[f][k[1]] += 1
                    if k[1] == "direct hit":
                        delay[f].append((m[k] - base - k[0] * 1000) / 1e6)
        area_hits = defaultdict(list)
        for k in feeds.get("area", {}):
            if k[1] == "direct hit":
                area_hits[k[2]].append(k[0])
        starts = [(k[0], k[2]) for k, _ in evs if k[1] == "cast start"]
        for t, sid in starts:
            hs = [h for h in area_hits.get(sid, []) if t <= h <= t + 3000]
            casts_hit[1] += 1
            if hs:
                casts_hit[0] += 1
                first_gap.append((min(hs) - t) / 1000)
    print(f"session {sess.name}: {logs} golem logs while recording\n")
    print(f"{'event kind':<16}{'in log':>8}" + "".join(f"{f:>10}" for f in sorted(feeds)))
    for k, n in total.most_common():
        print(f"{k:<16}{n:>8}" + "".join(f"{found[f][k]:>10}" for f in sorted(feeds)))
    for f in sorted(delay):
        d = sorted(delay[f])
        if d:
            print(f"\n{f} feed, direct hits: arrival delay median {statistics.median(d):.2f} s, "
                  f"90% {d[int(len(d) * .9)]:.2f} s, max {d[-1]:.2f} s (relative to the fastest event)")
    if casts_hit[1]:
        g = sorted(first_gap)
        print(f"\ncasts with a hit of the same skill in the area feed within 3 s: {casts_hit[0]} of {casts_hit[1]}"
              + (f"; first hit {statistics.median(g):.2f} s after the cast start (median)" if g else ""))


if __name__ == "__main__":
    main()
