"""A running pull as `evtc_rot.read()` gives a log, built from the live arcdps feed, so that any module
judges a live pull with the code that judges its logs (core/live_server.py).

    record(events, elite)          the record; `events` as the addon sends them (below)
    TABLE                          per skill, how a cast is put back from its hits (tools/livecast.py)

The live feed carries your hits, buff changes and condition ticks, about 2.65 s late, but no cast
starts or stops and no weapon swaps (addon/README.md). So casts are put back from hits: a skill's
first hit lands `offset` ms after its cast starts (the median over your logs), further hits of it
within `window` ms belong to the same cast, and for a pulsing field only those on its `pulse`.
Skills that do no damage show by the buff they put on you (`buff`, with a second one `gap` ms
later when the first alone also comes from elsewhere). A cast gets your median length
of that skill (`dur`), or less when the next cast starts sooner. Skills that never hit and put no buff on you (heals, dodges) are not seen.

Events are your own (source = you), oldest first, each {"time", "sc" (state change), "buff",
"skill", "value", "buff_dmg", "result", "dst_self"} with arcdps times in ms. arcdps starts a log at
the pull's first hit, so that hit is time 0 here; a cast that began before it is "under_way" (as in
a log, where only its end is seen) and is not among the casts.

A module can adjust the record in a `live(record)` function (the Berserker puts back its weapon
swaps); without one the record is used as it is.
"""
import json
from pathlib import Path

TABLE = Path(__file__).resolve().parent / "livecasts.json"
PULSE_TOL = 60
_table = {}


def table(elite=None):
    """{skill: rule} for a build: the cast table, read again when the file changes. A skill several
    builds cast has a rule per build ("23275/43": a Scrapper's dodge) that wins over a plain one."""
    m = TABLE.stat().st_mtime
    if _table.get("m") != m:
        _table.update(m=m, raw=json.loads(TABLE.read_text(encoding="utf-8")))
    out, own = {}, {}
    for k, v in _table["raw"].items():
        sid, _, e = k.partition("/")
        if not e:
            out[int(sid)] = v
        elif int(e) == elite:
            own[int(sid)] = v
    return {**out, **own}


def direct(e):
    """A direct hit of yours on a target (not a condition tick, not on yourself)."""
    return e["sc"] == 0 and e["buff"] == 0 and not e.get("activation") and not e.get("buffremove") \
        and not e.get("dst_self") and e.get("dst", 1) != 0


def casts_from(events, tab):
    """[{"id", "t", "at": [hit times]}] from your hits and the buffs you get, by the table."""
    casts, open_ = [], {}
    by_buff = {}
    for sid, r in tab.items():
        if r.get("buff"):
            by_buff.setdefault(r["buff"], []).append(sid)
    applies = {}
    for e in events:
        if e["sc"] == 69 and e.get("dst_self"):
            applies.setdefault(e["skill"], []).append(e["time"])
    for e in events:
        if e["sc"] == 69 and e.get("dst_self") and e["skill"] in by_buff:
            for sid in by_buff[e["skill"]]:
                r = tab[sid]
                # a pair: the second buff `gap` ms after the first (Superspeed comes from more than one skill)
                if r.get("with") and not any(abs(x - e["time"] - r["gap"]) <= 60 for x in applies.get(r["with"], [])):
                    continue
                casts.append({"id": sid, "t": e["time"] - r["offset"], "at": []})
                break
            continue
        if not direct(e):
            continue
        sid, t = e["skill"], e["time"]
        r = tab.get(sid)
        if r is None or r.get("buff"):
            continue
        op = open_.setdefault(sid, [])
        op[:] = [x for x in op if t - x[0] <= r["window"]]
        p = r["pulse"]
        same = next((x for x in op if not p or abs((t - x[0] + PULSE_TOL) % p - PULSE_TOL) <= PULSE_TOL), None)
        if same:
            same[1]["at"].append(t)
            continue
        c = {"id": sid, "t": t - r["offset"], "at": [t]}
        op.append((t, c))
        casts.append(c)
    return sorted(casts, key=lambda c: c["t"])


def record(events, elite=None, tab=None):
    """The live pull as `evtc_rot.read()` gives a log; None before the first hit."""
    tab = tab if tab is not None else table(elite)
    first = next((e["time"] for e in events if direct(e)), None)
    if first is None:
        return None
    casts, under_way = [], {}
    found = casts_from(events, tab)
    for k, c in enumerate(found):
        r = tab[c["id"]]
        # your usual length of the skill, cut by the next cast (86% of casts within 50 ms; cancelled
        # ones are off until the log comes)
        dur, t = r.get("dur") or 300, c["t"] - first
        if k + 1 < len(found) and 0 < found[k + 1]["t"] - c["t"] < dur:
            dur = found[k + 1]["t"] - c["t"]
        if t < 0:
            if t + dur > 0 and c["id"] not in under_way:
                under_way[c["id"]] = (t + dur, dur)
            continue
        # as evtc_rot counts them: "hits" up to 1 s past the cast's end, "during" 50 ms, "all" every one
        end = c["t"] + dur
        n = lambda tail: sum(1 for h in c["at"] if h <= end + tail)  # noqa: E731
        casts.append({"id": c["id"], "name": r["name"], "t": t, "exp": dur, "base": dur, "dur": dur, "stop": "done",
                      "hits": n(1000), "during": n(50), "all": len(c["at"]), "missed": 0, "dmg": 0})
    hits_at, buffs, gone, lost, init = {}, {}, {}, {}, set()
    damage, last_hit, end = 0, None, 0
    for e in events:
        t = e["time"] - first
        # what you had when the pull started: a log lists it as initial buffs (state change 18)
        if e["sc"] == 18 and e.get("dst_self"):
            init.add(e["skill"])
        if t < 0:
            if e["sc"] == 69 and e.get("dst_self"):
                init.add(e["skill"])
            elif e["sc"] == 72 and e.get("dst_self"):
                init.discard(e["skill"])
            continue
        end = max(end, t)
        if e["sc"] == 0 and not e.get("dst_self") and not e.get("buffremove"):
            d = e["value"] if e["buff"] == 0 else e["buff_dmg"]
            if d > 0:
                damage += d
                last_hit = t
                if e["buff"] == 0:
                    hits_at.setdefault(e["skill"], []).append(t)
        if e["sc"] in (69, 72) and e.get("dst_self"):
            (buffs if e["sc"] == 69 else gone).setdefault(e["skill"], []).append(t)
        if e["sc"] == 72:
            lost.setdefault(e["skill"], []).append(t)
    return {"player": "", "elite": elite, "casts": casts, "swaps": [], "buffs": buffs, "buffs_gone": gone,
            "loose": {}, "end": end, "file": "live", "hits_at": hits_at, "damage": damage,
            "last_hit": last_hit, "under_way": under_way, "buffs_init": sorted(init), "buffs_lost": lost,
            "names": {sid: r["name"] for sid, r in tab.items()}}
