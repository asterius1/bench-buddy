"""Helpers the build modules share. Take what fits; a module that needs something else writes its own.

    bench_casts(log)          the bench's casts from its Elite Insights JSON (`web.bench_log`)
    bench_buffs(log, ids)     the bench's buff intervals from the same JSON
    match(mine, ref, ordered) your casts lined up with the bench's: matched, missed and extra
    summary(p, ...)           one history row per log, cached next to the module until it changes
    icons(...)                skill icons from the bench and the GW2 API, cached
    sgn, sec, clk, tone       numbers as the pages and the addon show them

Times are ms. A cast is a dict with at least "id", "name", "t", "dur" and "cut" (a click cancelled
before it did anything, kept for display but left out of the matching).
"""
import difflib
import json

import web

CUT_MS = 100   # shorter casts are cancelled clicks, not casts


# ---- the bench ------------------------------------------------------------------------------

def bench_casts(log, cut_ms=CUT_MS, names=None):
    """Every cast of the log's first player, in time order, from `players[0].rotation`. Zero-length
    entries (instants, kit swaps: your own log has no animation for them) are left out. EI marks a
    cast cancelled before it did anything with timeGained = -duration: "cut", stop "cancel"."""
    pl, sm = log["players"][0], log["skillMap"]
    names = names or {}
    casts = []
    for r in pl["rotation"]:
        for s in r["skills"]:
            if s["duration"] <= 0:
                continue
            cancelled = s.get("timeGained", 0) < 0 and -s["timeGained"] >= s["duration"]
            casts.append({"id": r["id"], "name": names.get(r["id"], sm.get(f"s{r['id']}", {}).get("name", str(r["id"]))),
                          "t": s["castTime"], "dur": s["duration"], "hits": None,
                          "cut": s["duration"] < cut_ms or cancelled, "stop": "cancel" if cancelled else ""})
    casts.sort(key=lambda c: c["t"])
    return casts


def bench_buffs(log, ids, start=0):
    """{buff id: [(from, to), ...]} while the first player had it (`buffUptimes` states), clipped to
    start at `start`; an id the log doesn't have gets an empty list."""
    pl, end = log["players"][0], log["durationMS"]
    out = {}
    for b in pl["buffUptimes"]:
        if b["id"] in ids:
            st = b["states"] + [[end, 0]]
            out[b["id"]] = [(max(t1, start), t2) for (t1, n), (t2, _) in zip(st, st[1:]) if n > 0]
    for k in ids:
        out.setdefault(k, [])
    return out


# ---- matching -------------------------------------------------------------------------------

def match(mine, ref, ordered):
    """Line up your casts with the bench's (cut casts left out of the matching). Returns display
    items in order: {"c": yours, "r": its bench cast or None (extra)}, {"ghost": a bench cast you
    missed}. `ordered`: by sequence (difflib), else each of yours takes the first unused bench cast
    of the same skill. Your cut casts go back in by time with no bench cast."""
    m = [c for c in mine if not c["cut"]]
    r = [c for c in ref if not c["cut"]]
    out = []
    if ordered:
        sm = difflib.SequenceMatcher(None, [c["id"] for c in r], [c["id"] for c in m], autojunk=False)
        for op, i1, i2, j1, j2 in sm.get_opcodes():
            if op == "equal":
                out += [{"c": m[j1 + x], "r": r[i1 + x]} for x in range(i2 - i1)]
            else:
                out += [{"ghost": x} for x in r[i1:i2]] + [{"c": x, "r": None} for x in m[j1:j2]]
    else:
        pool = list(r)
        for c in m:
            hit = next((x for x in pool if x["id"] == c["id"]), None)
            if hit:
                pool.remove(hit)
            out.append({"c": c, "r": hit})
        out += [{"ghost": x} for x in pool]
    cuts = [c for c in mine if c["cut"]]
    res = []
    for it in out:
        if "c" in it:
            while cuts and cuts[0]["t"] < it["c"]["t"]:
                res.append({"c": cuts.pop(0), "r": None})
        res.append(it)
    res += [{"c": c, "r": None} for c in cuts]
    return res


# ---- caches ---------------------------------------------------------------------------------

def summary(p, cache, version, make):
    """One history row for log `p`: `make(p)` (a dict, or None to leave the log out), kept in
    `cache/<log>.json` until the log grows or `version` changes (bump it when the row changes)."""
    cache.mkdir(parents=True, exist_ok=True)
    f = cache / f"{p.stem}.json"
    if f.exists():
        d = json.loads(f.read_text(encoding="utf-8"))
        if d.get("v") == version and d.get("size") == p.stat().st_size:
            return d
    row = make(p)
    if row is None:
        return None
    d = {"v": version, "size": p.stat().st_size, **row}
    f.write_text(json.dumps(d), encoding="utf-8")
    return d


def icons(cache, skill_map, fill=(), drop=(), alias=None):
    """{skill id as text: icon url}: the bench's `skillMap` icons, minus `drop`, plus the `fill` ids
    it lacks from the GW2 API (skipped when offline), plus `alias` {new id: copied from id}. Kept in
    the `cache` file; delete it to fetch again."""
    if cache.exists():
        return json.loads(cache.read_text(encoding="utf-8"))
    out = {k[1:]: v["icon"] for k, v in skill_map.items() if v.get("icon")}
    for d in drop:
        out.pop(str(d), None)
    missing = [i for i in fill if str(i) not in out]
    if missing:
        try:
            for s in web.skills(missing):
                out[str(s["id"])] = s.get("icon", "")
        except OSError:
            pass
    for new, old in (alias or {}).items():
        if str(old) in out:
            out[str(new)] = out[str(old)]
    cache.parent.mkdir(parents=True, exist_ok=True)
    cache.write_text(json.dumps(out), encoding="utf-8")
    return out


# ---- numbers as shown -----------------------------------------------------------------------

def sgn(ms):
    """+0.12 / −0.05 / ±0.00 seconds; "" for None."""
    if ms is None:
        return ""
    return ("+" if ms > 0 else "−" if ms < 0 else "±") + f"{abs(ms) / 1000:.2f}"


def sec(ms):
    """0.68 s; — for None."""
    return "—" if ms is None else f"{ms / 1000:.2f} s"


def clk(ms):
    """1:05.3 from the fight start; — for None."""
    if ms is None:
        return "—"
    neg, s = ms < 0, abs(ms) / 1000
    return ("−" if neg else "") + f"{int(s // 60)}:{s % 60:04.1f}"


def tone(ms, near=50):
    """0 neutral, 1 ahead (green), 2 behind (red): a difference of more than `near` ms counts."""
    if ms is None:
        return 0
    return 2 if ms > near else 1 if ms < -near else 0
