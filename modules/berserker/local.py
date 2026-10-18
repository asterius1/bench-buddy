"""Your Condi Berserker golem pulls against the bench (the reference), for the checker (core/serve.py).

    pull(path)   one pull: visits (swap to swap) with every cast judged, lag behind the reference
    history()    one summary row per pull, cached in cache/local/<log>.json
    compare(path) one pull section by section against the reference row of the same number
    damage(path)  the opening seconds: damage and condition stacks against the reference
    icons()      skill icons, from the reference log and the GW2 API, cached in cache/icons.json
"""
from pathlib import Path

import baseline
import evtc_rot
import kit
import web
from evtc_rot import evtc

HERE = Path(__file__).resolve().parent
REF_LOG = HERE / "cache" / "log.json"
PERMALINK = "YPVM-20260720-095643_golem"   # the bench: Snow Crows' Condition Berserker golem log on dps.report
CACHE = HERE / "cache" / "local"
ICONS = HERE / "cache" / "icons.json"
VERSION = 1  # bump when the summary changes, to recompute the cache

SWORD = {14366, 29845, 80247, 80224, 29940, 30682, 14364, 14365, 14363}
BOW = {29923, 30189, 14519, 14504, 14381, 14431, 14505, 14522}
_ref = None
REF_FIRST = [0]   # the reference's first cast, its time zero (as yours is your first cast)


def ref_log():
    """The bench's Elite Insights JSON, downloaded from dps.report the first time."""
    return web.bench_log(PERMALINK, REF_LOG)


def ref():
    """(per skill reference, reference swap times in ms from its first cast)."""
    global _ref
    if _ref is None:
        log = ref_log()
        pl = log["players"][0]
        first = min(s["castTime"] for r in pl["rotation"] if r["id"] != -2 for s in r["skills"]
                    if s["duration"] > 0 and s["castTime"] >= 0)
        swaps = sorted(s["castTime"] - first for r in pl["rotation"] if r["id"] == -2 for s in r["skills"])
        _ref = (baseline.reference(log), swaps)
        REF_FIRST[0] = first
    return _ref


def pull(path):
    rf, ref_swaps = ref()
    r = evtc_rot.read(path)
    casts = r["casts"]
    if not casts:
        return None
    t0 = casts[0]["t"]
    bounds = [t0] + [s["t"] for s in r["swaps"] if s["t"] > t0] + [r["end"]]
    visits, fails, judged = [], {}, 0
    for a, b in zip(bounds, bounds[1:]):
        cs = []
        for c in casts:
            if not a <= c["t"] < b:
                continue
            v = baseline.judge(c, rf, r["buffs"])
            if v != "cut":
                judged += 1
                for why in v:
                    fails.setdefault(c["id"], {"name": baseline.NAMES.get(c["id"], c["name"]),
                                               "few": 0, "long": 0, "nobuff": 0})[why] += 1
            cs.append({"id": c["id"], "name": baseline.NAMES.get(c["id"], c["name"]), "t": c["t"] - t0,
                       "dur": c["dur"], "hits": c["hits"], "stop": c["stop"],
                       "v": v if v == "cut" else v,
                       "limit": rf[c["id"]]["max"] + baseline.SLACK_MS if c["id"] in rf else None,
                       "expect": baseline.EXPECT.get(c["id"], (None,))[0]})
        ids = {c["id"] for c in cs}
        weapon = "sword" if ids & SWORD else "bow" if ids & BOW else ""
        visits.append({"start": a - t0, "len": b - a, "weapon": weapon, "casts": cs, "last": b == r["end"]})
    mine_swaps = [s["t"] - t0 for s in r["swaps"] if s["t"] > t0]
    lag = [m - f for m, f in zip(mine_swaps, ref_swaps)]
    ref_len = [b - a for a, b in zip([0] + ref_swaps, ref_swaps)]
    n_fail = sum(f["few"] + f["long"] + f["nobuff"] for f in fails.values())
    sw = [v["len"] / 1000 for v in visits[1:] if v["weapon"] == "sword" and not v["last"]]
    bw = [v["len"] / 1000 for v in visits[1:] if v["weapon"] == "bow" and not v["last"]]
    return {"log": r["file"], "secs": round((r["end"] - t0) / 1000, 1), "visits": visits, "lag": lag,
            "refLen": ref_len, "fails": fails, "judged": judged, "failed": n_fail,
            "berserk": baseline.uptime(r, baseline.BERSERK),
            "sword": baseline.stats(sw), "bow": baseline.stats(bw), "player": r["player"]}


def _summary(p):
    x = pull(p)
    if x is None:
        return None
    return {"log": x["log"], "secs": x["secs"],
            "loops": sum(1 for v in x["visits"] if v["weapon"] == "sword"), "judged": x["judged"],
            "failed": x["failed"], "berserk": x["berserk"], "sword": x["sword"], "bow": x["bow"],
            "lag3": x["lag"][5] if len(x["lag"]) > 5 else None}


def summary(p):
    """One history row, cached per log."""
    return kit.summary(p, CACHE, VERSION, _summary)


def history():
    return [s for s in (summary(p) for p in evtc_rot.golem_logs()) if s]


def icons():
    # the stow icon on the render server is broken
    return kit.icons(ICONS, ref_log()["skillMap"], fill=(14505, 14522, 14363, 14364, 14365), drop=(baseline.STOW,))


# ---- section by section comparison ---------------------------------------------------------

SLOW_MS = 50          # a cast this much longer than the reference cast it lines up with is flagged
SECTIONS = ["a", "b", "open", "fill", "close"]
FLURRY_PART = {29940, 30682}           # the sword visit's second section starts at FoW or Flurry
CLOSE = {29923, 30189}                 # SE and BR before the closing Fan of Fire
FOF = 14519
INSTANT = {-2, 30258, 31289, 30435, 9433, 9441}  # swaps, Outrage, King of Fires, Berserk, sigils
_refrows = None


def _sections(casts, weapon, opener=False):
    """Split one visit's casts into named sections. Cut casts go with the section they fall in."""
    real = [c for c in casts if not c["cut"]]
    if opener:
        return {"open": casts}
    if weapon == "sword":
        i = next((k for k, c in enumerate(real) if c["id"] in FLURRY_PART), len(real))
        tb = real[i]["t"] if i < len(real) else float("inf")
        return {"a": [c for c in casts if c["t"] < tb], "b": [c for c in casts if c["t"] >= tb]}
    ids = [c["id"] for c in real]
    if FOF not in ids:
        return {"open": casts}
    i = ids.index(FOF) + 1
    j = len(ids)
    if ids[-1] == FOF and len(ids) - 1 >= i:
        j = len(ids) - 1
        while j - 1 >= i and ids[j - 1] in CLOSE:
            j -= 1
    t_open = real[i - 1]["t"]
    t_close = real[j]["t"] if j < len(real) else float("inf")
    return {"open": [c for c in casts if c["t"] <= t_open],
            "fill": [c for c in casts if t_open < c["t"] < t_close],
            "close": [c for c in casts if c["t"] >= t_close]}


def _rows(casts, swaps, end):
    """Rows of sections: row 0 the opener visit, then one row per sword visit plus the longbow
    visit after it. A section's time runs from its first cast to the next section's first cast
    (the swap for the last one)."""
    first = casts[0]["t"] if casts else 0
    bounds = [first] + [t for t in swaps if t > first] + [end]
    visits = []
    for a, b in zip(bounds, bounds[1:]):
        cs = [c for c in casts if a <= c["t"] < b]
        ids = {c["id"] for c in cs if not c["cut"]}
        weapon = "sword" if ids & SWORD else "bow" if ids & BOW else ""
        visits.append({"weapon": weapon, "casts": cs, "start": a, "end": b, "last": b == end})
    rows = []
    for k, v in enumerate(visits):
        opener = k == 0 and v["weapon"] != "sword"
        secs = _sections(v["casts"], v["weapon"], opener=opener)
        starts = [(n, min((c["t"] for c in secs[n] if not c["cut"]), default=None)) for n in SECTIONS if n in secs]
        starts = [(n, t) for n, t in starts if t is not None]
        times = {}
        for (n, t), nxt in zip(starts, starts[1:] + [(None, None)]):
            if nxt[1] is not None:
                times[n] = nxt[1] - t
            else:
                times[n] = None if v["last"] else v["end"] - t
        item = {"weapon": v["weapon"], "sections": secs, "times": times,
                "len": None if v["last"] else v["end"] - v["start"], "start": v["start"]}
        if opener:
            rows.append({"opener": item})
        elif v["weapon"] == "sword" or not rows or "bow" in rows[-1] or "opener" in rows[-1]:
            rows.append({"sword": item} if v["weapon"] == "sword" else {"bow": item})
        else:
            rows[-1]["bow"] = item
    return rows


def ref_rows():
    global _refrows
    if _refrows is None:
        log = ref_log()
        pl = log["players"][0]
        casts, swaps = [], []
        for r in pl["rotation"]:
            for s in r["skills"]:
                if r["id"] == -2:
                    swaps.append(s["castTime"])
                elif r["id"] not in INSTANT and s["duration"] > 0:
                    casts.append({"id": r["id"], "t": s["castTime"], "dur": s["duration"], "hits": None,
                                  "stop": "", "cut": s["duration"] < baseline.CUT_MS and r["id"] != baseline.STOW,
                                  "name": baseline.NAMES.get(r["id"], log["skillMap"][f"s{r['id']}"]["name"])})
        casts.sort(key=lambda c: c["t"])
        _link_next(casts)
        _refrows = _rows(casts, sorted(swaps), log["durationMS"])
    return _refrows


def _link_next(casts):
    """Give each real cast the start of the next real cast (cut autos skipped: their time counts
    as waiting) and when that next cast could have started at the earliest: right away, or later
    when it is Scorched Earth or Fan of Fire still on cooldown (CD_MS after its previous cast ended;
    Blood Reckoning resets Scorched Earth). Waiting for a cooldown is then not counted as a wait."""
    real = [c for c in casts if not c["cut"]]
    last_end = {}
    for a, b in zip(real, real[1:]):
        a["next"] = b["t"]
    for c in real:
        ready = None
        if c["id"] in CD_MS and c["id"] in last_end:
            ready = last_end[c["id"]] + CD_MS[c["id"]]
        c["ready"] = ready
        if c["id"] == BR:
            last_end.pop(SE, None)
        if c["id"] in CD_MS and c["dur"] is not None:
            last_end[c["id"]] = c["t"] + c["dur"]
    for a, b in zip(real, real[1:]):
        a["next_ready"] = b.get("ready")


def _ref_for(k, refs):
    """The reference row for row k: the same number; past the reference's last full row, the row
    one cycle earlier (6 loops: Flames of War every 2nd, Blood Reckoning every 3rd)."""
    while k >= len(refs) - 1 and k > 6:   # the reference's last row is cut by the log end
        k -= 6
    return refs[k] if k < len(refs) else None


def _chip(it, rf, buffs, t0):
    if "ghost" in it:
        g = it["ghost"]
        return {"id": g["id"], "name": g["name"], "kind": "ghost", "refDur": g["dur"], "refT": g["t"] - REF_FIRST[0]}
    c, r = it["c"], it["r"]
    out = {"id": c["id"], "name": c["name"], "t": c["t"] - t0, "dur": c["dur"], "hits": c["hits"],
           "stop": c["stop"], "expect": baseline.EXPECT.get(c["id"], (None,))[0]}
    if c["cut"]:
        out["kind"] = "cut"
        return out
    v = baseline.judge(c, rf, buffs)
    flags = [f for f in v if f != "long"] if v != "cut" else []
    out["kind"] = "ok" if r else "extra"
    if r:
        out["ref"] = {"id": r["id"], "name": r["name"], "dur": r["dur"], "t": r["t"] - REF_FIRST[0]}
    base = r["dur"] if r else (rf[c["id"]]["med"] if c["id"] in rf else None)
    if base is not None and c["dur"] is not None:
        out["delta"] = c["dur"] - base          # cast time alone, for the card; not judged
        out["refDur"] = base
    # Judged: the slot, from this cast's start to the next real cast's start, against the bench's
    # slot at the same point. Where a cast ends and its aftercast or the wait begins does not
    # matter (an auto cut short then waited out is the same as one played out); the next skill
    # being still on cooldown is not a loss, so the next cast is expected at the later of the
    # bench's pace and that skill's ready time. A closing cast before a swap is judged by when
    # the first skill on the other weapon starts.
    if c.get("next") is not None and c["dur"] is not None:
        end = c["t"] + c["dur"]
        out["slot"] = c["next"] - c["t"]
        out["gap"] = c["next"] - max(end, c.get("next_ready") or end)
        if c.get("next_ready") and c["next_ready"] > end:
            out["cdWait"] = c["next_ready"] - end      # the next skill was still on cooldown
        if r and r.get("next") is not None:
            out["refSlot"] = r["next"] - r["t"]
            expect = max(c["t"] + out["refSlot"], c.get("next_ready") or 0)
            out["lost"] = c["next"] - expect
            if out["lost"] > SLOW_MS:
                flags.append("slow")
    out["flags"] = flags
    return out


# Scorched Earth and Fan of Fire come back 4.0 s after the end of their cast (5 s with alacrity):
# the shortest end-to-start recast in 81 of your SE pairs and 62 FoF pairs is 4.00 to 4.07 s.
CD_MS = {29923: 4000, 14519: 4000}
SE, FOF_ID, BR = 29923, 14519, 30189


def cooldown_window(sections):
    """The longbow visit's filler window. The close can only start when SE (and FoF) are ready again:
    4.0 s after the end of the opening SE / FoF. Returns when SE is ready (ms from the window start,
    which is the end of the opening), how late the closing SE and FoF started after being ready, the
    filler time used, and the time inside the window spent on nothing."""
    op = [c for c in sections.get("open", []) if not c["cut"]]
    fill = [c for c in sections.get("fill", []) if not c["cut"]]
    cl = [c for c in sections.get("close", []) if not c["cut"]]
    se_o = [c for c in op if c["id"] == SE]
    fof_o = [c for c in op if c["id"] == FOF_ID]
    if not se_o or not op:
        return None
    w0 = max(c["t"] + c["dur"] for c in op)                      # end of the opening
    se_ready = se_o[-1]["t"] + se_o[-1]["dur"] + CD_MS[SE]
    fof_ready = fof_o[-1]["t"] + fof_o[-1]["dur"] + CD_MS[FOF_ID] if fof_o else None
    se_c = next((c for c in cl if c["id"] == SE), None)
    fof_c = next((c for c in cl if c["id"] == FOF_ID), None)
    used = sum(c["dur"] for c in fill)
    return {"budget": se_ready - w0, "used": used,
            "idle": (min(se_c["t"], se_ready) - w0 - used) if se_c else None,
            "seLate": se_c["t"] - se_ready if se_c else None,
            "fofLate": fof_c["t"] - fof_ready if fof_c and fof_ready else None,
            "closed": se_c is not None}


def _refchips(rcs):
    return [{"id": x["id"], "name": x["name"], "dur": x["dur"]} for x in rcs if not x["cut"]]


def compare(path):
    rf, _ = ref()
    refs = ref_rows()
    r = evtc_rot.read(path)
    if not r["casts"]:
        return None
    casts = [{**c, "name": baseline.NAMES.get(c["id"], c["name"]),
              "cut": baseline.judge(c, rf, r["buffs"]) == "cut"} for c in r["casts"]]
    _link_next(casts)
    t0 = casts[0]["t"]
    mine = _rows(casts, [s["t"] for s in r["swaps"]], r["end"])
    rows = []
    for k, row in enumerate(mine):
        rr = _ref_for(k, refs) or {}
        out = {"n": k, "refN": refs.index(rr) if rr else None}
        for part in ("opener", "sword", "bow"):
            if part not in row:
                continue
            mv, rv = row[part], rr.get(part)
            secs = {}
            for name, cs in mv["sections"].items():
                rcs = (rv or {}).get("sections", {}).get(name, [])
                items = kit.match(cs, rcs, ordered=(name != "fill"))
                secs[name] = {"chips": [_chip(it, rf, r["buffs"], t0) for it in items],
                              "time": mv["times"].get(name), "refTime": (rv or {}).get("times", {}).get(name),
                              "ref": _refchips(rcs)}
            for name, rcs in (rv or {}).get("sections", {}).items():
                if name not in secs:
                    secs[name] = {"chips": [{"id": x["id"], "name": x["name"], "kind": "ghost", "refDur": x["dur"],
                                             "refT": x["t"] - REF_FIRST[0]}
                                            for x in rcs if not x["cut"]],
                                  "time": None, "refTime": rv["times"].get(name), "ref": _refchips(rcs)}
            out[part] = {"sections": secs, "len": mv["len"], "refLen": (rv or {}).get("len"),
                         "start": mv["start"] - t0}
            if part == "bow":
                out[part]["cd"] = {"mine": cooldown_window(mv["sections"]),
                                   "ref": cooldown_window(rv["sections"]) if rv else None}
        rows.append(out)
    p = pull(path)
    return {"log": r["file"], "rows": rows, "lag": p["lag"], "secs": p["secs"], "failed": p["failed"],
            "judged": p["judged"], "berserk": p["berserk"], "sword": p["sword"], "bow": p["bow"],
            "slow": SLOW_MS, "damage": damage(path) if r["file"] != "live" else None}


SWAP_MS = 116   # a live swap: this long before the first cast on the other weapon (median of 286 swaps)


def live(r):
    """The live record (core/live.py) made ready for compare(): casts get the bench's median length
    (live casts have none; what is judged doesn't depend on it), an auto attack with fewer hits than
    a full one counts as cancelled, as in a log, and the weapon swaps the feed doesn't carry are put
    back SWAP_MS before the first cast on the other weapon."""
    rf, _ = ref()
    for c in r["casts"]:
        if c["id"] in rf:
            c["exp"] = c["base"] = c["dur"] = round(rf[c["id"]]["med"])
        c["name"] = baseline.NAMES.get(c["id"], c["name"])
        if c["id"] in baseline.AUTOS and c["id"] in baseline.EXPECT and c["hits"] < baseline.EXPECT[c["id"]][0]:
            c["stop"] = "cancel"
    weapon = None
    for a, b in zip(r["casts"], r["casts"][1:]):
        wa = "sword" if a["id"] in SWORD else "bow" if a["id"] in BOW else weapon
        wb = "sword" if b["id"] in SWORD else "bow" if b["id"] in BOW else wa
        weapon = wa
        if wa and wb and wa != wb:
            r["swaps"].append({"t": max(a["t"] + 1, b["t"] - SWAP_MS), "set": 0})
    r["end"] = max([r["end"]] + [c["t"] + c["dur"] for c in r["casts"]])
    return r


# ---- the opening seconds: damage and condition stacks against the reference -------------------

GOLEM_SPECIES = 16199
CONDS = {"Burning": "Burning", "Bleeding": "Bleeding", "Torment": "Torment", "Poison": "Poisoned"}
WINDOWS = [(0, 5), (5, 10), (10, 15), (15, 20), (20, 25), (25, 30)]
OPENING_S = 30
_refdmg = None


def _avg_stacks(intervals, a, b):
    return round(sum(max(0, min(y, b) - max(x, a)) for x, y in intervals) / (b - a), 1)


def ref_damage():
    """Cumulative damage per second and condition stacks per window in the reference."""
    global _refdmg
    if _refdmg is None:
        log = ref_log()
        pl, tg, bm = log["players"][0], log["targets"][0], log["buffMap"]
        cum = pl["targetDamage1S"][0][0][:OPENING_S + 1]
        stacks = {}
        for c in CONDS:
            b = next(x for x in tg["buffs"] if bm.get(f"b{x['id']}", {}).get("name") == c)
            st = b["statesPerSource"][pl["name"]] + [[10 ** 9, 0]]
            iv = [(t1 / 1000, t2 / 1000) for (t1, n), (t2, _) in zip(st, st[1:]) for _ in range(n)]
            stacks[c] = [_avg_stacks(iv, a, b_) for a, b_ in WINDOWS]
        buffs = {b["id"]: bm[f"b{b['id']}"]["name"] for b in pl["buffUptimes"] if f"b{b['id']}" in bm}
        _refdmg = {"cum": cum, "stacks": stacks, "buffs": buffs}
    return _refdmg


def damage(path):
    """Your cumulative damage on the golem per second from your first hit, condition stacks per
    window (from apply events and their durations), and the reference's self buffs you lack."""
    agents, skills, events = evtc.load(path)
    me = next(a for a in agents.values() if a.is_player)
    golem = {a.addr for a in agents.values() if a.species == GOLEM_SPECIES}
    hits = [e.time for e in events if e.sc == 0 and e.src == me.addr and e.dst in golem
            and (e.value > 0 or e.buff_dmg > 0)]
    if not hits:
        return None
    first = hits[0]
    per = [0] * (OPENING_S + 1)
    iv = {c: [] for c in CONDS}
    names = {v: k for k, v in CONDS.items()}
    mine_buffs = set()
    for e in events:
        if e.dst == me.addr and e.sc in (18, 69):
            mine_buffs.add(e.skill)
        if e.src != me.addr or e.dst not in golem:
            continue
        t = (e.time - first) / 1000
        if e.sc == 0:
            dmg = e.value if e.buff == 0 else e.buff_dmg
            if dmg > 0 and 0 <= t < OPENING_S + 1:
                per[int(t)] += dmg
        elif e.sc == 69 and skills.get(e.skill) in names:
            iv[names[skills[e.skill]]].append((t, t + e.value / 1000))
    cum, tot = [], 0
    for x in per:
        tot += x
        cum.append(tot)
    rd = ref_damage()
    # the reference's arrays are cumulative at each second mark, ours through the end of each second
    return {"mine": [0] + cum[:OPENING_S], "ref": rd["cum"], "until": (hits[-1] - first) / 1000,
            "windows": WINDOWS, "stacks": {c: {"mine": [_avg_stacks(iv[c], a, b) for a, b in WINDOWS],
                                               "ref": rd["stacks"][c]} for c in CONDS},
            "missingBuffs": [n for i, n in rd["buffs"].items() if i not in mine_buffs]}
