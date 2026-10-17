"""Your Power Scrapper golem pulls against the Snow Crows bench, for the checker (core/serve.py).

    compare(path)   one pull, cycle by cycle against the bench cycle of the same number
    logs()          your scrapper golem logs, oldest first
    history()       one summary row per pull

The loop: the 20 s skills (16 s with alacrity) are the anchor and should keep
rolling. Each cycle holds three mini loops, each opened by Electro-whirl + Shrapnel Grenade, with
filler after it; the third one ends in the bomb burst (Medic Gyro, Big Ol' Bomb, Magnetic Bomb ...
Thunderclap, Mine Field), and the next cycle starts at the next Electro-whirl.

Casts: yours from the raw log (core/evtc_rot.py: animation start/stop), the bench's from its Elite
Insights JSON (modules/scrapper/cache/bench.json, dps.report 55xd-20260422-192633_golem). Kit swaps and
instant skills have no animation in your log, so both sides drop zero-length casts.
"""
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
for d in (HERE.parent.parent / "core", HERE.parent / "berserker"):
    sys.path.insert(0, str(d))
import evtc_rot  # noqa: E402
import web  # noqa: E402
import local  # noqa: E402  (the section matching is shared with the Berserker checker)
from evtc_rot import evtc  # noqa: E402

BENCH = HERE / "cache" / "bench.json"
PERMALINK = "55xd-20260422-192633_golem"
ICONS = HERE / "cache" / "icons.json"
CACHE = HERE / "cache" / "local"
VERSION = 1

SCRAPPER = 43
EW, SHRAP, TC = 30088, 5807, 30713
FREEZE, POISON, MEDIC, BOB, MAGNET = 5809, 5806, 30357, 5813, 76530
FGYRO, BARRAGE, MINEFIELD, THROWMINE = 56920, 5810, 6164, 6161
GALVANIC, FIREBOMB, PGS, ENDO = 5822, 5823, 30885, 30307
HAMMER = {30501, 29785, 30489}            # Positive Strike, Negative Bash, Equalizing Blow
BURST = {MEDIC, BOB, MAGNET}               # the bomb burst opens the third mini loop's last section
TWENTY = [FREEZE, POISON, MEDIC, BOB, MAGNET, TC]   # 20 s recharge (GW2 API)
# Recharge in seconds (GW2 API), counted from the end of the cast: the bench recasts the 20 s skills
# 16.12 to 16.36 s after the previous cast ended, never under 16.
CD = {TC: 20, FREEZE: 20, POISON: 20, MEDIC: 20, BOB: 20, MAGNET: 20, FGYRO: 25, BARRAGE: 25,
      MINEFIELD: 17, THROWMINE: 12, GALVANIC: 16, FIREBOMB: 8, PGS: 10, ENDO: 15, EW: 6, SHRAP: 5}
TRACKED = TWENTY[:-1] + [TC, FGYRO, BARRAGE, MINEFIELD]   # the cooldowns shown per cycle
ALACRITY = 30328
SUPERSPEED, SWIFTNESS, STABILITY = 5974, 719, 1122
FERVOR, VOLATILITY = 76326, 76128
FERVOR_S = 8
DODGE = 23275
CUT_MS = 100
SLOW_MS = 50
NAMES = {DODGE: "Dodge"}
GOLEM_SPECIES = 16199


# Hits a cast lands until the skill's next cast: the usual count in your logs (completed casts, 24 pulls),
# the same as the bench's hits per cast where it has them. Mine Field is 5 mines; Thunderclap 5 pulses.
HITS = {EW: 2, SHRAP: 3, FREEZE: 3, POISON: 3, BARRAGE: 6, FGYRO: 1, FIREBOMB: 4, PGS: 1, TC: 5, BOB: 1,
        MAGNET: 1, GALVANIC: 1, MINEFIELD: 5, THROWMINE: 1, ENDO: 1, 30501: 1, 29785: 1, 30489: 1}
HIT_TAIL_MS = 4000   # casts this close to the end of the pull may not have landed yet: not judged


# ---- reading --------------------------------------------------------------------------------

def logs():
    return evtc_rot.golem_logs(elite=SCRAPPER, needs=(EW, SHRAP))


def _intervals(applies, gones, start, end, initial=False):
    """On intervals of a buff from apply and all-stacks-gone times."""
    ev = sorted([(t, 1) for t in applies] + [(t, 0) for t in gones])
    out, since = [], (start if initial else None)
    for t, kind in ev:
        if kind and since is None:
            since = t
        elif not kind and since is not None:
            out.append((since, t))
            since = None
    if since is not None:
        out.append((since, end))
    return out


def read_mine(path):
    """Your casts and buff intervals (ms from the log start; t0 is your first cast)."""
    r = evtc_rot.read(path)
    agents, skills, events = evtc.load(path)
    me = next(a for a in agents.values() if a.is_player and a.name == r["player"])
    golem = {a.addr for a in agents.values() if a.species == GOLEM_SPECIES}
    first = events[0].time
    app, gone, init = {}, {}, set()
    dmg, last_hit = 0, None
    hits, started, stop_only = {}, set(), {}
    for e in events:
        t = e.time - first
        if e.src == me.addr and e.sc == 67:
            started.add(e.skill)
        elif e.src == me.addr and e.sc == 68 and e.skill not in started and e.skill not in stop_only:
            stop_only[e.skill] = (t, e.value)        # cast under way when the log started
        if e.sc == 0 and e.buff == 0 and e.src == me.addr and e.dst in golem and e.value > 0:
            hits.setdefault(e.skill, []).append(t)
        if e.sc == 18 and e.dst == me.addr:
            init.add(e.skill)
        elif e.sc == 69 and e.dst == me.addr:
            app.setdefault(e.skill, []).append(t)
        elif e.sc == 72 and e.src == me.addr:
            gone.setdefault(e.skill, []).append(t)
        elif e.sc == 0 and e.src == me.addr and e.dst in golem:
            d = e.value if e.buff == 0 else e.buff_dmg
            if d > 0:
                dmg += d
                last_hit = t
    casts = []
    for c in r["casts"]:
        dur = c["dur"] if c["dur"] is not None else c["exp"]
        casts.append({"id": c["id"], "name": NAMES.get(c["id"], c["name"]), "t": c["t"], "dur": dur,
                      "hits": c["hits"], "all": c["all"], "cut": dur < CUT_MS, "stop": c["stop"]})
    # A hammer auto cancelled before it landed (0 hits, stopped early) costs nothing but its time: it is
    # left out of the matching, like the bench's cancelled autos. Any other skill cancelled before it
    # landed stays a cast and shows as missed hits (a Galvanic Bomb cut at 0.2 to 0.37 s is never placed).
    for c in casts:
        if not c["cut"] and c["id"] in HAMMER and c["all"] == 0 and c["stop"] != "done":
            c["cut"] = True
            c["cancelled"] = True
    for c in casts:
        c["endOk"] = c["t"] + c["dur"] + HIT_TAIL_MS < (last_hit if last_hit is not None else r["end"])
    casts = _precasts(casts, hits, stop_only, skills) + casts
    t0 = 0      # the fight start (the log starts at the first hit)
    end = last_hit if last_hit is not None else r["end"]
    buffs = {b: _intervals(app.get(b, []), gone.get(b, []), 0, end, b in init)
             for b in (ALACRITY, SUPERSPEED, SWIFTNESS, STABILITY, FERVOR)}
    return {"log": r["file"], "player": r["player"], "casts": casts, "t0": t0, "end": end,
            "buffs": buffs, "damage": dmg}


# Bombs go off a fixed fuse after they are placed (seen in play: Big Ol' Bomb 3 s, Magnetic 2, Galvanic and
# Fire Bomb 1); used when the log has no cast of the skill to measure cast start to first hit from.
FUSE = {BOB: 3000, MAGNET: 2000, GALVANIC: 1000, FIREBOMB: 1000}


def _precasts(casts, hits, stop_only, skills):
    """Casts before the fight. The log starts at the first hit, so skills cast before it have no
    animation start. A cast still under way has its end (start = end - duration). A bomb that hits
    before its first logged cast was placed before the fight: its start is the first hit minus the usual
    time from cast start to first hit in this log (the fuse). Fire Bomb hits 4 times, 1 s apart: fewer
    in the first seconds means the missing first tick was the hit that started the fight (not logged).
    Mines only hit when detonated, so pre-placed mines go before everything, in the order players
    play it (Throw Mine, Mine Field, Big Ol' Bomb, Magnetic, Fire Bomb, Galvanic, Thunderclap)."""
    from statistics import median
    out, mines = [], []
    for sid, (t_end, dur) in stop_only.items():
        if sid in KIT:
            out.append({"id": sid, "name": NAMES.get(sid, skills.get(sid, str(sid))), "t": t_end - dur, "dur": dur,
                        "hits": None, "cut": False, "stop": "", "pre": "end"})
    for sid, hs in hits.items():
        if sid not in KIT or sid in stop_only or sid in HAMMER or sid in (EW,):
            continue
        mine = [c for c in casts if c["id"] == sid and not c["cut"]]
        if mine and mine[0]["t"] <= hs[0]:
            continue
        durs = [c["dur"] for c in mine] or [600]
        d = round(median(durs))
        item = {"id": sid, "name": NAMES.get(sid, skills.get(sid, str(sid))), "t": 0, "dur": d, "hits": None,
                "cut": False, "stop": "", "pre": "hit", "hit": hs[0]}
        if sid in (THROWMINE, MINEFIELD):
            item["pre"] = "order"
            mines.append(item)
            continue
        first = hs[0]
        if sid == FIREBOMB:
            early = [h for h in hs if h < hs[0] + 3500]
            if len(early) < FIRE_TICKS:
                first = hs[0] - 1000 * (FIRE_TICKS - len(early))
                item["firstHit"] = True     # its missing tick started the fight
        delays = []
        for c, nxt in zip(mine, mine[1:] + [None]):
            h = next((x for x in hs if x >= c["t"] and (nxt is None or x < nxt["t"])), None)
            if h is not None:
                delays.append(h - c["t"])
        delay = median(delays) if delays else FUSE.get(sid, 0) + d
        item.update({"t": min(first - delay, -d), "hit": first, "delay": round(delay)})
        out.append(item)
    out.sort(key=lambda c: c["t"])
    mines.sort(key=lambda c: OPENER.index(c["id"]))
    t = out[0]["t"] if out else 0
    for c in reversed(mines):
        t -= c["dur"]
        c["t"] = t
    out = mines + out
    # one cast at a time: push an earlier cast back so it ends before the next one starts
    seq = out + [c for c in casts if not c["cut"]][:1]
    for a, b in reversed(list(zip(seq, seq[1:]))):
        if a["t"] + a["dur"] > b["t"]:
            a["t"] = b["t"] - a["dur"]
    return out


_bench = None
FIRE_TICKS = 4    # Fire Bomb hits 4 times per cast, 1 s apart (every cast in your logs)
OPENER = [THROWMINE, MINEFIELD, BOB, MAGNET, FIREBOMB, GALVANIC]   # the usual pre-fight order


def _bench_precasts(pl, casts):
    """The bench log is Elite Insights JSON: hits only as totals per skill. A bomb or mine with one
    hit more than its casts (Big Ol' Bomb, Magnetic, Galvanic, Throw Mine: one hit per cast; Fire
    Bomb: 4 per cast), or a detonation before the first Mine Field, was placed before the fight. Their
    times are unknown: back to back in that order, ending where the first logged cast starts."""
    from statistics import median
    hits = {t["id"]: t["hits"] for t in pl["totalDamageDist"][0]}
    det = min((s["castTime"] for r in pl["rotation"] if r["id"] in (-37, 6166, 6162) for s in r["skills"]), default=None)
    by = {}
    for c in casts:
        by.setdefault(c["id"], []).append(c)
    pre = []
    for sid in OPENER:
        cs = by.get(sid, [])
        per = FIRE_TICKS if sid == FIREBOMB else 1
        extra = hits.get(sid, 0) > per * len(cs) if sid != MINEFIELD else \
            (det is not None and (not cs or det < cs[0]["t"]))
        if extra:
            d = round(median(c["dur"] for c in cs)) if cs else 600
            pre.append({"id": sid, "name": cs[0]["name"] if cs else str(sid), "t": 0, "dur": d, "hits": None,
                        "cut": False, "stop": "", "pre": "count"})
    t = casts[0]["t"] if casts else 0
    for c in reversed(pre):
        t -= c["dur"]
        c["t"] = t
    return pre


def read_bench():
    global _bench
    if _bench is None:
        log = web.bench_log(PERMALINK, BENCH)
        pl, sm = log["players"][0], log["skillMap"]
        casts = []
        for r in pl["rotation"]:
            for s in r["skills"]:
                if s["duration"] <= 0:
                    continue
                # EI marks a cast cancelled before it did anything with timeGained = -duration
                cancelled = s.get("timeGained", 0) < 0 and -s["timeGained"] >= s["duration"]
                casts.append({"id": r["id"], "name": sm.get(f"s{r['id']}", {}).get("name", str(r["id"])),
                              "t": s["castTime"], "dur": s["duration"], "hits": None,
                              "cut": s["duration"] < CUT_MS or cancelled, "stop": "cancel" if cancelled else ""})
        casts.sort(key=lambda c: c["t"])
        casts = _bench_precasts(pl, casts) + casts
        end = log["durationMS"]
        buffs = {}
        for b in pl["buffUptimes"]:
            if b["id"] in (ALACRITY, SUPERSPEED, SWIFTNESS, STABILITY, FERVOR):
                st = b["states"] + [[end, 0]]
                buffs[b["id"]] = [(max(t1, casts[0]["t"]), t2) for (t1, n), (t2, _) in zip(st, st[1:]) if n > 0]
        for k in (ALACRITY, SUPERSPEED, SWIFTNESS, STABILITY, FERVOR):
            buffs.setdefault(k, [])
        _bench = {"log": PERMALINK, "player": pl["name"], "casts": casts, "t0": 0, "end": end,
                  "buffs": buffs, "damage": pl["dpsTargets"][0][0]["damage"],
                  "icons": {k[1:]: v["icon"] for k, v in sm.items() if v.get("icon")}}
    return _bench


# ---- cooldowns -------------------------------------------------------------------------------

def _on(iv, t):
    return any(a <= t < b for a, b in iv)


def _ready(end, cd, alac):
    """When a skill whose cast ended at `end` is ready: its recharge runs 25% faster under alacrity."""
    t, left = end, cd
    for a, b in sorted(alac) + [(10 ** 12, 10 ** 12)]:
        if b <= t:
            continue
        if a > t:                       # without alacrity until a
            if t + left <= a:
                return t + left
            left -= a - t
            t = a
        span = (b - t) * 1.25           # with alacrity until b
        if left <= span:
            return t + left / 1.25
        left -= span
        t = b
    return t + left


def _link(casts, alac):
    """Each real cast gets the next real cast's start and when that one was ready (its recharge
    counted from the end of its previous cast, 25% faster under alacrity)."""
    real = [c for c in casts if not c["cut"]]
    last_end = {}
    for c in real:
        c["ready"] = None
        if c["id"] in CD and c["id"] in last_end:
            e = last_end[c["id"]]
            c["ready"] = _ready(e, CD[c["id"]] * 1000, alac)
        if c["id"] in CD:
            last_end[c["id"]] = c["t"] + c["dur"]
    for a, b in zip(real, real[1:]):
        a["next"] = b["t"]
        a["next_ready"] = b["ready"]
    return real


# ---- cycles and mini loops ---------------------------------------------------------------------

def _cycles(casts, end):
    """[{"pre": [...], "minis": [[sections]...]}] split at every Electro-whirl; a mini loop with a
    bomb-burst skill closes its cycle. Sections: core (Electro-whirl through Shrapnel), fill, burst."""
    real = [c for c in casts if not c["cut"]]
    ews = [c["t"] for c in real if c["id"] == EW]
    if not ews:
        return []
    bounds = ews + [end + 1]
    pre = [c for c in casts if c["t"] < ews[0]]
    minis = []
    for a, b in zip(bounds, bounds[1:]):
        cs = [c for c in casts if a <= c["t"] < b]
        rs = [c for c in cs if not c["cut"]]
        i = next((k for k, c in enumerate(rs) if c["id"] == SHRAP), 0)
        t_fill = rs[i + 1]["t"] if i + 1 < len(rs) else None
        # in the mini loop with the bomb burst, everything after Shrapnel is the burst
        j = i + 1 if any(c["id"] in BURST for c in rs[i + 1:]) else None
        t_burst = rs[j]["t"] if j is not None and j < len(rs) else None
        secs = {"core": [c for c in cs if t_fill is None or c["t"] < t_fill]}
        if t_fill is not None:
            secs["fill"] = [c for c in cs if t_fill <= c["t"] < (t_burst or b)]
        if t_burst is not None:
            secs["burst"] = [c for c in cs if c["t"] >= t_burst]
            secs.pop("fill", None)
        times = {}
        starts = [(n, min(c["t"] for c in v if not c["cut"])) for n, v in secs.items()
                  if any(not c["cut"] for c in v)]
        last = b > end
        for (n, t), nx in zip(starts, starts[1:] + [(None, None)]):
            times[n] = (nx[1] - t) if nx[1] is not None else (None if last else b - t)
        minis.append({"sections": secs, "times": times, "start": a, "len": None if last else b - a,
                      "burst": t_burst is not None})
    # A cycle starts at the whirl with the bomb burst (column 0), then the grenade filler whirl and the
    # other filler whirl. Before the first bomb-burst whirl, the opener takes column 0.
    cycles, cur = [], []
    for m in minis:
        if m["burst"] and cur:
            cycles.append(cur)
            cur = []
        cur.append(m)
    if cur:
        cycles.append(cur)
    if cycles and not cycles[0][0]["burst"]:
        cycles[0] = [None] + cycles[0]
    out = []
    for k, ms in enumerate(cycles):
        start = ms[0]["start"] if ms[0] else 0
        nxt = cycles[k + 1][0]["start"] if k + 1 < len(cycles) else None
        out.append({"minis": ms, "pre": pre if k == 0 else [], "start": start,
                    "len": (nxt - start) if nxt is not None else None})
    return out


def _cooldowns(cyc, alac):
    """Per tracked skill in the cycle: when it was cast, when it was ready, how late."""
    out = []
    cs = [c for m in cyc["minis"] if m for v in m["sections"].values() for c in v if not c["cut"]] + \
         [c for c in cyc["pre"] if not c["cut"]]
    for sid in TRACKED:
        for c in sorted((c for c in cs if c["id"] == sid), key=lambda c: c["t"]):
            out.append({"id": sid, "name": c["name"], "t": c["t"], "ready": c.get("ready"),
                        "late": (c["t"] - c["ready"]) if c.get("ready") is not None else None})
    return sorted(out, key=lambda c: c["t"])


def _bench_for(k, refs):
    """The bench cycle for cycle k: the same number; past the bench's last full cycle, two earlier
    (Function Gyro and Grenade Barrage, 25 s, alternate between cycles)."""
    while k >= len(refs) - 1 and k > 2:
        k -= 2
    return refs[k] if k < len(refs) else None


def _chip(it, t0, rt0):
    if "ghost" in it:
        g = it["ghost"]
        return {"id": g["id"], "name": g["name"], "kind": "ghost", "refDur": g["dur"], "refT": g["t"] - rt0}
    c, r = it["c"], it["r"]
    out = {"id": c["id"], "name": c["name"], "t": c["t"] - t0, "dur": c["dur"], "hits": c.get("hits"),
           "stop": c.get("stop"), "flags": []}
    if c.get("pre"):
        out["pre"] = c["pre"]
        out["hit"] = c.get("hit")
    if c["cut"]:
        out["kind"] = "cut"
        return out
    out["kind"] = "ok" if r else "extra"
    exp = HITS.get(c["id"])
    if exp and c.get("all") is not None and c.get("endOk", True):
        out["landed"], out["expect"] = c["all"], exp
        if c["all"] < exp:
            out["flags"].append("few")
    if r:
        out["ref"] = {"id": r["id"], "name": r["name"], "dur": r["dur"], "t": r["t"] - rt0}
        out["delta"] = c["dur"] - r["dur"]
        out["refDur"] = r["dur"]
    if c.get("ready") is not None:
        out["late"] = c["t"] - c["ready"]
        if r and r.get("ready") is not None:
            out["refLate"] = r["t"] - r["ready"]
    if c.get("next") is not None:
        end = c["t"] + c["dur"]
        out["slot"] = c["next"] - c["t"]
        if c.get("next_ready") and c["next_ready"] > end:
            out["cdWait"] = c["next_ready"] - end
        if r and r.get("next") is not None:
            out["refSlot"] = r["next"] - r["t"]
            expect = max(c["t"] + out["refSlot"], c.get("next_ready") or 0)
            out["lost"] = c["next"] - expect
            if out["lost"] > SLOW_MS:
                out["flags"].append("slow")
    return out


def _uptime(iv, a, b):
    return sum(max(0, min(y, b) - max(x, a)) for x, y in iv) / (b - a) if b > a else None


def _motion(buffs, a, b, step=100):
    """Object in Motion: +5% strike damage per boon among stability, swiftness and superspeed,
    compounding. Average bonus and boon count from a to b."""
    n = tot = cnt = 0
    t = a
    while t < b:
        k = sum(_on(buffs[x], t) for x in (STABILITY, SWIFTNESS, SUPERSPEED))
        tot += 1.05 ** k - 1
        cnt += k
        n += 1
        t += step
    return {"bonus": round(100 * tot / n, 2) if n else None, "boons": round(cnt / n, 2) if n else None}


def _buffs(side, a, b):
    bf = side["buffs"]
    gaps = []
    for (x1, y1), (x2, y2) in zip(bf[SUPERSPEED], bf[SUPERSPEED][1:]):
        if a <= y1 < b and x2 - y1 > 300:
            gaps.append({"from": y1 - side["t0"], "len": x2 - y1})
    if bf[SUPERSPEED] and bf[SUPERSPEED][-1][1] < b - 300:
        gaps.append({"from": bf[SUPERSPEED][-1][1] - side["t0"], "len": b - bf[SUPERSPEED][-1][1]})
    return {"superspeed": round(100 * _uptime(bf[SUPERSPEED], a, b)),
            "stability": round(100 * _uptime(bf[STABILITY], a, b)),
            "swiftness": round(100 * _uptime(bf[SWIFTNESS], a, b)),
            "alacrity": round(100 * _uptime(bf[ALACRITY], a, b)),
            "fervor": round(100 * _uptime(bf[FERVOR], a, b)),
            "motion": _motion(bf, a, b), "ssGaps": gaps}


def _fervor(side, cyc):
    """When Bloodstone Fervor came up in this cycle and which 20 s skills it covered."""
    a = cyc["start"]
    b = a + (cyc["len"] or 10 ** 9)
    iv = [(x, y) for x, y in side["buffs"][FERVOR] if a - 2000 <= x < b - 2000]
    cs = [c for m in cyc["minis"] if m for v in m["sections"].values() for c in v if not c["cut"]] + \
        [c for c in cyc["pre"] if not c["cut"]]
    hard = [c for c in cs if c["id"] in TWENTY]
    inside = [c["name"] for c in hard if any(x <= c["t"] < y for x, y in side["buffs"][FERVOR])]
    return {"at": [x - a for x, _ in iv], "inside": inside, "of": len(hard)}


def _rows(side):
    casts = [dict(c) for c in side["casts"]]
    _link(casts, side["buffs"][ALACRITY])
    return _cycles(casts, side["end"])


_brows = None


def bench_rows():
    global _brows
    if _brows is None:
        _brows = _rows(read_bench())
    return _brows


def compare(path):
    me = read_mine(path)
    if not me["casts"]:
        return None
    bench = read_bench()
    refs = bench_rows()
    mine = _rows(me)
    t0, rt0 = me["t0"], bench["t0"]
    rows = []
    for k, cy in enumerate(mine):
        rc = _bench_for(k, refs)
        row = {"n": k, "refN": refs.index(rc) if rc else None, "start": cy["start"] - t0, "len": cy["len"],
               "refLen": rc["len"] if rc else None, "minis": []}
        if cy["pre"] or (rc and rc["pre"] and k == 0):
            items = local._match(cy["pre"], rc["pre"] if rc and k == 0 else [], ordered=True)
            row["pre"] = {"chips": [_chip(it, t0, rt0) for it in items],
                          "ref": [{"id": c["id"], "name": c["name"], "dur": c["dur"], "t": c["t"] - rt0}
                                  for c in (rc["pre"] if rc and k == 0 else []) if not c["cut"]]}
        for i, m in enumerate(cy["minis"]):
            rm = rc["minis"][i] if rc and i < len(rc["minis"]) else None
            if m is None:
                row["minis"].append(None)
                continue
            secs = {}
            names = list(m["sections"]) + [n for n in (rm or {}).get("sections", {}) if n not in m["sections"]]
            for n in ("core", "fill", "burst"):
                if n not in names:
                    continue
                cs = m["sections"].get(n, [])
                rcs = (rm or {}).get("sections", {}).get(n, [])
                items = local._match(cs, rcs, ordered=(n == "core"))
                secs[n] = {"chips": [_chip(it, t0, rt0) for it in items], "time": m["times"].get(n),
                           "refTime": (rm or {}).get("times", {}).get(n),
                           "ref": [{"id": c["id"], "name": c["name"], "dur": c["dur"], "t": c["t"] - rt0} for c in rcs if not c["cut"]]}
            row["minis"].append({"sections": secs, "len": m["len"], "refLen": rm["len"] if rm else None,
                                 "start": m["start"] - t0, "refStart": rm["start"] - rt0 if rm else None})
        cdm = _cooldowns(cy, me["buffs"][ALACRITY])
        cdr = _cooldowns(rc, bench["buffs"][ALACRITY]) if rc else []
        for c in cdm:
            same = [x for x in cdr if x["id"] == c["id"]]
            if same:
                c["refLate"] = same[0]["late"]
                cdr.remove(same[0])
            c["t"] -= t0
            if c["ready"] is not None:
                c["ready"] -= t0
        row["cd"] = cdm
        row["cdMissing"] = [{"id": x["id"], "name": x["name"]} for x in cdr]
        lates = [c["late"] for c in cdm if c["id"] in TWENTY and c["late"] is not None]
        rl = [c["refLate"] for c in cdm if c["id"] in TWENTY and c.get("refLate") is not None]
        row["late20"] = round(sum(lates) / len(lates)) if lates else None
        row["refLate20"] = round(sum(rl) / len(rl)) if rl else None
        tc = next((c for c in cdm if c["id"] == TC), None)
        row["tc"] = tc and {"late": tc["late"], "refLate": tc.get("refLate")}
        row["fervor"] = {"mine": _fervor(me, cy), "ref": _fervor(bench, rc) if rc else None}
        a = cy["start"]
        b = a + cy["len"] if cy["len"] else me["end"]
        row["buffs"] = _buffs(me, a, b)
        if rc:
            ra = rc["start"]
            row["refBuffs"] = _buffs(bench, ra, ra + rc["len"] if rc["len"] else bench["end"])
        rows.append(row)
    # lag: your Electro-whirls against the bench's, from both first casts
    ew_m = [c["t"] - t0 for c in me["casts"] if c["id"] == EW and not c["cut"]]
    ew_r = [c["t"] - rt0 for c in bench["casts"] if c["id"] == EW and not c["cut"]]
    secs = (me["end"] - t0) / 1000
    bsecs = (bench["end"] - rt0) / 1000
    return {"log": me["log"], "player": me["player"], "rows": rows, "secs": round(secs, 1),
            "benchSecs": round(bsecs, 1), "dps": round(me["damage"] / secs) if secs > 0 else None,
            "benchDps": round(bench["damage"] / bsecs), "damage": me["damage"],
            "lag": [m - r for m, r in zip(ew_m, ew_r)], "ews": ew_m, "slow": SLOW_MS,
            "buffs": _buffs(me, t0, me["end"]), "refBuffs": _buffs(bench, rt0, bench["end"]),
            "twenty": [str(x) for x in TWENTY],
            "fervorIv": [[a - t0, b - t0] for a, b in me["buffs"][FERVOR]]}


# ---- the bench rotation as a cheat sheet ------------------------------------------------------

KIT = {SHRAP: "grenade", FREEZE: "grenade", POISON: "grenade", BARRAGE: "grenade",
       FIREBOMB: "bomb", GALVANIC: "bomb", BOB: "bomb", MAGNET: "bomb",
       PGS: "mortar", ENDO: "mortar", FGYRO: "gyro", MEDIC: "gyro", THROWMINE: "mine", MINEFIELD: "mine",
       6162: "mine", 6166: "mine", -37: "mine", EW: "hammer", TC: "hammer", **{h: "hammer" for h in HAMMER}}
SHOWN_INSTANT = {6162: "Detonate", 6166: "Detonate Mine Field", -37: "Detonate", 76326: "Bloodstone Fervor"}
REGULAR = 0.75     # a skill in at least this share of full cycles' same mini loop is part of the usual loop


def _tokens(casts, instants):
    """One mini loop as display tokens: Electro-whirl + Shrapnel as one, hammer auto chains as one,
    everything else a skill. Instants (detonations, Fervor) are marks on the token they happen during,
    or the next one, so they never split a chain."""
    real = sorted([c for c in casts if not c["cut"]], key=lambda c: c["t"])
    out = []
    for c in real:
        if c["id"] == SHRAP and out and out[-1]["kind"] == "ew" and "shrap" not in out[-1]:
            out[-1]["shrap"] = c["t"]
            out[-1]["end"] = c["t"] + c["dur"]
            continue
        if c["id"] == EW:
            out.append({"kind": "ew", "id": EW, "name": "Electro-whirl + Shrapnel", "t": c["t"], "end": c["t"] + c["dur"]})
        elif c["id"] in HAMMER:
            if out and out[-1]["kind"] == "aa":
                out[-1]["n"] += 1
                out[-1]["end"] = c["t"] + c["dur"]
                out[-1]["ids"].append(c["id"])
            else:
                out.append({"kind": "aa", "id": 30501, "name": "Hammer auto chain", "t": c["t"],
                            "end": c["t"] + c["dur"], "n": 1, "ids": [c["id"]]})
        else:
            out.append({"kind": "skill", "id": c["id"], "name": c["name"], "t": c["t"], "dur": c["dur"],
                        "end": c["t"] + c["dur"],
                        "late": c["t"] - c["ready"] if c.get("ready") is not None else None,
                        "cd": CD.get(c["id"])})
    for x in out:
        x["kit"] = KIT.get(x["id"], "")
        x["marks"] = []
    for i in sorted(instants, key=lambda c: c["t"]):
        mark = {"id": i["id"], "name": SHOWN_INSTANT[i["id"]], "t": i["t"], "kit": KIT.get(i["id"], "")}
        host = next((x for x in out if x["t"] <= i["t"] < x["end"]), None)             or next((x for x in out if x["t"] >= i["t"]), None) or (out[-1] if out else None)
        if host is not None:
            host["marks"].append(mark)
    return out


def bench_sheet():
    """The bench's cycles: rows of three mini loops (Electro-whirl to Electro-whirl) as tokens, each
    skill marked usual or not against the same mini loop in the other full cycles."""
    b = read_bench()
    log = json.loads(BENCH.read_text(encoding="utf-8"))
    sm = log["skillMap"]
    inst = [{"id": r["id"], "name": SHOWN_INSTANT[r["id"]], "t": s["castTime"], "dur": 0, "cut": False}
            for r in log["players"][0]["rotation"] if r["id"] in SHOWN_INSTANT for s in r["skills"]]
    rows = bench_rows()
    t0 = b["t0"]
    full = [r for r in rows if r["len"] is not None]
    out = []
    for k, cy in enumerate(rows):
        minis = []
        for i, m in enumerate(cy["minis"]):
            if m is None:
                minis.append(None)
                continue
            end = m["start"] + (m["len"] or 10 ** 9)
            cs = [c for v in m["sections"].values() for c in v]
            toks = _tokens(cs, [x for x in inst if m["start"] <= x["t"] < end])
            minis.append({"start": m["start"] - cy["start"], "len": m["len"], "tokens": toks,
                          "burst": m["burst"]})
        pre = _tokens(cy["pre"], [x for x in inst if x["t"] < cy["start"]]) if cy["pre"] else []
        out.append({"n": k, "start": cy["start"] - t0, "len": cy["len"], "minis": minis, "pre": pre})
    # usual skills per mini loop position, from the full cycles
    usual = []
    for i in range(3):
        cnt = {}
        for r in full:
            ms = out[rows.index(r)]["minis"]
            if i < len(ms) and ms[i]:
                for x in {t["id"] for t in ms[i]["tokens"] if t["kind"] == "skill"} |                         {k["id"] for t in ms[i]["tokens"] for k in t["marks"]}:
                    cnt[x] = cnt.get(x, 0) + 1
        usual.append({x for x, n in cnt.items() if n >= REGULAR * len(full)})
    for r in out:
        for i, m in enumerate(r["minis"]):
            if m is None:
                continue
            have = {t["id"] for t in m["tokens"]} | {k["id"] for t in m["tokens"] for k in t["marks"]}
            for t in m["tokens"]:
                t["usual"] = t["kind"] in ("ew", "aa") or t["id"] in usual[i]
            m["missing"] = [{"id": x, "name": sm.get(f"s{x}", {}).get("name", SHOWN_INSTANT.get(x, str(x))),
                             "kit": KIT.get(x, "")}
                            for x in sorted(usual[i]) if x not in have and x not in SHOWN_INSTANT] if r["len"] is not None else []
    toks = [t for r in out for m in r["minis"] if m for t in m["tokens"]] + [t for r in out for t in r["pre"]]
    for t in toks + [k for t in toks for k in t["marks"]]:
        t["t"] -= t0
        if "end" in t:
            t["end"] -= t0
        if "shrap" in t:
            t["shrap"] -= t0
    # each mini loop's filler: what's left once its fixed part is shown in the structure on top.
    # Bomb burst: only its floating skills. Nade and other filler: from the end of the leading run of
    # usual casts (whirl + Shrapnel, Freeze and Poison).
    for r in out:
        for i, m in enumerate(r["minis"]):
            if m is None:
                continue
            ts = m["tokens"]
            if i == 0:
                m["filler"] = [t for t in ts if t["kind"] == "skill" and not t["usual"]]
            else:
                j = 0
                while j < len(ts) and (ts[j]["kind"] == "ew" or (ts[j]["kind"] == "skill" and ts[j]["usual"])):
                    j += 1
                m["filler"] = ts[j:]
    order = {}
    for r in full:
        for i, m in enumerate(out[rows.index(r)]["minis"]):
            for j, t in enumerate(m["tokens"] if m else []):
                if t["id"] in usual[i]:
                    order.setdefault((i, t["id"]), []).append(j)
    usual_list = [sorted(({"id": x, "name": sm.get(f"s{x}", {}).get("name", SHOWN_INSTANT.get(x, str(x))),
                           "kit": KIT.get(x, "")} for x in usual[i]),
                         key=lambda d: sum(order.get((i, d["id"]), [99])) / len(order.get((i, d["id"]), [1])))
                  for i in range(3)]
    # the structure: the opener, then each mini loop's fixed casts from the first full cycle that has it, with a
    # "filler" slot where its floating skills go
    slot = {"kind": "slot", "id": 0, "name": "filler", "kit": "", "usual": True, "marks": []}
    structure = [{"name": "Opener", "tokens": out[0]["pre"] if out else []}]
    for i, name in enumerate(["Bomb burst", "Nade filler", "Other filler"]):
        m = next((out[rows.index(r)]["minis"][i] for r in full
                  if i < len(out[rows.index(r)]["minis"]) and out[rows.index(r)]["minis"][i]), None)
        ts = m["tokens"] if m else []
        fill = {id(t) for t in m["filler"]} if m else set()
        fixed, at = [], None
        for t in ts:
            if id(t) in fill:
                at = len(fixed) if at is None else at
            elif t["kind"] != "aa":
                fixed.append(t)
        structure.append({"name": name, "tokens": fixed[:at] + [slot] + fixed[at:] if at is not None else fixed + [slot]})
    return {"rows": out, "usual": usual_list, "structure": structure, "player": b["player"], "log": PERMALINK,
            "secs": round((b["end"] - t0) / 1000, 1), "full": len(full)}


def summary(p):
    CACHE.mkdir(parents=True, exist_ok=True)
    f = CACHE / f"{p.stem}.json"
    if f.exists():
        d = json.loads(f.read_text(encoding="utf-8"))
        if d.get("v") == VERSION and d.get("size") == p.stat().st_size:
            return d
    x = compare(p)
    if x is None or not x["rows"]:
        return None
    d = {"v": VERSION, "size": p.stat().st_size, "log": x["log"], "secs": x["secs"], "dps": x["dps"],
         "cycles": len(x["rows"]), "lag": x["lag"][-1] if x["lag"] else None,
         "superspeed": x["buffs"]["superspeed"]}
    f.write_text(json.dumps(d), encoding="utf-8")
    return d


def history():
    return [s for s in (summary(p) for p in logs()) if s]


def icons():
    if ICONS.exists():
        return json.loads(ICONS.read_text(encoding="utf-8"))
    out = dict(read_bench()["icons"])
    missing = [i for i in (ENDO, DODGE, 31248, 6164, 6162, 6166) if str(i) not in out]
    try:
        for s in web.skills(missing):
                out[str(s["id"])] = s.get("icon", "")
    except OSError:
        pass
    if "6166" in out:
        out["-37"] = out["6166"]   # EI's merged "Detonate (Throw Mine / Mine Field)"
    ICONS.parent.mkdir(parents=True, exist_ok=True)
    ICONS.write_text(json.dumps(out), encoding="utf-8")
    return out


if __name__ == "__main__":
    ls = logs()
    print(len(ls), "scrapper logs")
    x = compare(ls[-1])
    print(x["log"], x["secs"], "s", x["dps"], "dps; bench", x["benchSecs"], x["benchDps"])
    print("buffs", x["buffs"], "\nbench", x["refBuffs"])
    for r in x["rows"]:
        print(f"cycle {r['n']} (bench {r['refN']}) start {r['start']/1000:.1f} len {r['len']} bench {r['refLen']}"
              f" late20 {r['late20']} bench {r['refLate20']} tc {r['tc']} ss {r['buffs']['superspeed']}%"
              f" fervor {r['fervor']['mine']}")
        for m in [m for m in r["minis"] if m]:
            print("   ", m["len"], m["refLen"], {n: (s["time"], s["refTime"], " ".join(
                (c["name"][:10] + ("!" if "slow" in c.get("flags", []) else "") + ("?" if c["kind"] == "ghost" else "")
                 + ("+" if c["kind"] == "extra" else "")) for c in s["chips"] if c["kind"] != "cut"))
                for n, s in m["sections"].items()})
        print("    cd", [(c["name"][:8], c["late"], c.get("refLate")) for c in r["cd"]], "missing", r["cdMissing"])
