"""One player's casts and hits from a raw arcdps log: what every module reads.

    from evtc_rot import read, golem_logs
    rec = read(path)        # {"player", "elite", "start", "casts": [...], "swaps": [...], "end"}

Casts come from animation start/stop (state changes 67 and 68; this arcdps logs no activation
events). Start: value is the expected duration with the current speed, buff_dmg the expected
duration at normal speed. Stop: value is the actual duration, result how it ended (STOP below).
Each direct hit (state change 0, not a buff tick) goes to the latest cast of the same skill id that
started before it. "hits" counts those up to 1 s past the cast's end (projectiles in flight; later
ones are field pulses such as Scorched Earth's), "during" those up to 50 ms past it, "all" every one
until the skill's next cast. Weapon swaps are state change 11, value the new set.
Times are ms from the first event of the log. "buffs" and "buffs_gone" are the times each buff
was applied to the player (state change 69) and fully removed (72), by buff id.
"""
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))  # evtc.py: the raw log reader
import evtc  # noqa: E402

# arcdps's log folder; set BENCH_BUDDY_LOGS when yours is somewhere else
LOGS = Path(os.environ.get("BENCH_BUDDY_LOGS") or Path.home() / "Documents" / "Guild Wars 2" / "addons" / "arcdps" / "arcdps.cbtlogs")
GOLEM = LOGS / "Standard Kitty Golem (16199)"
BERSERKER = 18  # elite spec id
STOP = {1: "instant", 3: "transition", 4: "partial", 5: "ended", 6: "cancel", 7: "stow", 8: "interrupt",
        11: "cc", 12: "next skill", 13: "motion", 14: "dodge", 16: "move skill", 17: "moved", 22: "done"}
# hit results that did not connect
MISSED = {3: "block", 4: "evade", 6: "absorb", 7: "blind"}
LATE_MS = 50
WINDOW_MS = 1000
FIRST_ANIMATED = "20261001"  # older logs carry no animation events


def golem_logs(elite=BERSERKER, needs=(29923, 30682)):
    """Standard golem logs recorded on a character of this elite spec that cast any of `needs`
    (default Scorched Earth or Flaming Flurry: the longbow / sword build), oldest first.
    Logs before October 2026 have no animation events, so no casts."""
    out = []
    for p in sorted(GOLEM.glob("*.zevtc")):
        if p.stem < FIRST_ANIMATED:
            continue
        try:
            agents, _, events = evtc.load(p)
        except Exception:
            continue
        mine = {a.addr for a in agents.values() if a.is_player and a.elite == elite}
        if mine and any(e.sc == 67 and e.src in mine and e.skill in needs for e in events):
            out.append(p)
    return out


def read(path):
    agents, skills, events = evtc.load(path)
    pov = next((e.src for e in events if e.sc == 13), None)
    me = agents.get(pov) or next(a for a in agents.values() if a.is_player)
    t0 = events[0].time
    casts, open_ = [], {}
    hits, swaps, buffs, gone = [], [], {}, {}
    for e in events:
        if e.dst == me.addr and e.sc in (69, 72):
            (buffs if e.sc == 69 else gone).setdefault(e.skill, []).append(e.time - t0)
        if e.src != me.addr:
            continue
        t = e.time - t0
        if e.sc == 67:
            c = {"id": e.skill, "name": skills.get(e.skill, str(e.skill)), "t": t, "exp": e.value,
                 "base": e.buff_dmg or e.value, "dur": None, "stop": None, "hits": 0, "during": 0, "all": 0,
                 "missed": 0, "dmg": 0}
            casts.append(c)
            open_[e.skill] = c
        elif e.sc == 68:
            c = open_.pop(e.skill, None)
            if c is not None:
                c["dur"] = e.value
                c["stop"] = STOP.get(e.result, str(e.result))
        elif e.sc == 11:
            swaps.append({"t": t, "set": e.value})
        elif e.sc == 0 and e.buff == 0 and e.dst != 0:
            hits.append((t, e.skill, e.value, e.result))
    by_skill = {}
    for c in casts:
        by_skill.setdefault(c["id"], []).append(c)
    loose = {}
    for t, sid, dmg, res in hits:
        cs = by_skill.get(sid)
        c = None
        if cs:
            for x in cs:
                if x["t"] <= t:
                    c = x
                else:
                    break
        if c is None:
            loose[sid] = loose.get(sid, 0) + 1
            continue
        c["all"] += 1
        end = c["t"] + (c["dur"] if c["dur"] is not None else c["exp"])
        if t > end + WINDOW_MS:
            continue
        c["hits"] += 1
        c["dmg"] += dmg
        if res in MISSED:
            c["missed"] += 1
        if t <= end + LATE_MS:
            c["during"] += 1
    return {"player": me.name, "elite": me.elite, "casts": casts, "swaps": swaps,
            "buffs": buffs, "buffs_gone": gone,
            "loose": {skills.get(k, str(k)): v for k, v in loose.items()},
            "end": events[-1].time - t0, "file": Path(path).stem}


if __name__ == "__main__":
    import collections
    for p in golem_logs()[-3:]:
        r = read(p)
        print(p.stem, r["player"], len(r["casts"]), "casts", len(r["swaps"]), "swaps", r["loose"])
        for c in r["casts"][:25]:
            print(f"  {c['t']:6} {c['name']:<22} exp {c['exp']:5} dur {c['dur']} {c['stop']:<11} "
                  f"hits {c['hits']} ({c['during']} during)")
        print(collections.Counter(c["stop"] for c in r["casts"]))
