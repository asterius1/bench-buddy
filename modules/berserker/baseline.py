"""Baseline per skill from the reference log, and the same checks on your own golem logs.

Reference: the Snow Crows log (Elite Insights JSON from dps.report). It has every cast with its
duration, but hits only as totals per skill, so hits per cast there are total / casts.
Yours: the raw logs, read by evtc_rot.py, with hits per cast.

A cast fails a check when
  - it hit fewer times than EXPECT (hits up to 1 s after the cast ends),
  - it took longer than the reference's longest cast of that skill plus SLACK_MS, or
  - it is a buff skill (BUFF) and the buff was not applied before the cast ended.
Auto attacks cut short are not failures (the reference cuts them too); they are counted apart.
"""
from collections import Counter
from statistics import median

import evtc_rot

SLACK_MS = 100
CUT_MS = 100  # shorter casts are cancelled clicks, not casts
# Hits a full cast should land within 1 s of its end, and where that number comes from.
EXPECT = {
    14366: (1, "ref"), 29845: (1, "ref"), 80247: (1, "ref"), 80224: (1, "ref"),
    30682: (6, "ref 47 in 8; guide: stow after the 6th hit"), 14364: (1, "ref"), 14365: (1, "ref"),
    14363: (1, "ref"), 29923: (1, "first hit; the field pulses later"), 14519: (3, "ref"),
    14504: (1, "ref"), 14381: (1, "ref"), 14431: (2, "ref"), 29613: (1, "ref"), 30074: (1, "ref"),
    30343: (1, "ref"), 14505: (1, "yours"),
}
AUTOS = {14364, 14365, 14363, 14431}
NO_HIT = {30189: "applies Blood Reckoning", 29940: "applies Flames of War (lands about 0.48 s in)"}
BUFF = {29940: 31708, 30189: 29466}   # skill: the buff it must apply
BERSERK = 29502
STOW = 23285
ORDER = [14366, 29845, 80247, 80224, 29940, 30682, 23285, 14364, 14365, 14363,
         29923, 30189, 14519, 14504, 14381, 14431, 14505, 29613, 30074, 30343, 14522]
NAMES = {80224: "Rend (2nd)", 23285: "Weapon stow", 14522: "Combustive Shot", 23275: "Dodge"}
SWORD_MARK, BOW_MARK = 14366, 29923   # a visit with Savage Leap is sword, with Scorched Earth longbow


def reference(log):
    """Per skill from the EI JSON: casts, cast time median and range, hits per cast."""
    pl = log["players"][0]
    totals = {t["id"]: t for t in pl["totalDamageDist"][0]}
    out = {}
    for r in pl["rotation"]:
        sid = r["id"]
        ds = [s["duration"] for s in r["skills"] if s["duration"] >= CUT_MS or (sid == STOW and s["duration"] > 0)]
        if not ds:
            continue
        hits = totals.get(sid, {}).get("connectedHits", 0)
        out[sid] = {"casts": len(r["skills"]), "full": len(ds), "med": median(ds), "min": min(ds),
                    "max": max(ds), "hits": hits}
    return out


def visits(times, ids_per_visit, end):
    """Sword and longbow visit lengths (swap to swap) in seconds."""
    sw, bow = [], []
    for (a, b), ids in zip(zip(times, times[1:] + [None]), ids_per_visit):
        if b is None:
            continue
        L = (b - a) / 1000
        if SWORD_MARK in ids:
            sw.append(L)
        elif BOW_MARK in ids:
            bow.append(L)
    return sw, bow


def stats(xs):
    if not xs:
        return None
    xs = sorted(xs)
    return {"n": len(xs), "med": round(median(xs), 2), "min": round(xs[0], 2), "max": round(xs[-1], 2)}


def judge(c, ref, buffs):
    """"cut" for an auto attack cut short or a click under CUT_MS, else the list of failed checks
    ("few", "long", "nobuff"); empty when the cast is fine."""
    sid, dur = c["id"], c["dur"] if c["dur"] is not None else 0
    if dur < CUT_MS and sid != STOW:
        return "cut"
    short_auto = sid in AUTOS and c["stop"] != "done"
    if short_auto and sid in EXPECT and c["hits"] < EXPECT[sid][0]:
        return "cut"
    out = []
    if sid in EXPECT and c["hits"] < EXPECT[sid][0]:
        out.append("few")
    if sid in ref and dur > ref[sid]["max"] + SLACK_MS and not short_auto:
        out.append("long")
    if sid in BUFF and not any(c["t"] <= t <= c["t"] + dur + 50 for t in buffs.get(BUFF[sid], [])):
        out.append("nobuff")
    return out


def mine(ref):
    """Run the checks over your logs. Returns per skill counts and examples, and visit lengths."""
    per = {}
    sw_all, bow_all, logs = [], [], []
    for p in evtc_rot.golem_logs():
        r = evtc_rot.read(p)
        logs.append(r["file"])
        for c in r["casts"]:
            sid = c["id"]
            d = per.setdefault(sid, {"casts": 0, "ok": 0, "long": [], "few": [], "nobuff": [], "cut": 0, "durs": [],
                                     "hits": Counter(), "name": c["name"]})
            d["casts"] += 1
            verdict = judge(c, ref, r["buffs"])
            if verdict == "cut":
                d["cut"] += 1
                continue
            dur = c["dur"] or 0
            d["durs"].append(dur)
            d["hits"][c["hits"]] += 1
            ex = {"log": r["file"], "t": round(c["t"] / 1000, 1), "dur": dur, "hits": c["hits"], "stop": c["stop"]}
            for why in verdict:
                d[why].append(ex)
            if not verdict:
                d["ok"] += 1
        # visits: casts between swaps
        sws = [s["t"] for s in r["swaps"]]
        ids = [[c["id"] for c in r["casts"] if a <= c["t"] < b]
               for a, b in zip(sws, sws[1:] + [float("inf")])]
        s, b = visits(sws, ids, r["end"])
        sw_all += s
        bow_all += b
        logs[-1] = {"log": r["file"], "secs": round(r["end"] / 1000, 1), "casts": len(r["casts"]),
                    "berserk": uptime(r, BERSERK), "sword": stats(s), "bow": stats(b)}
    rows = []
    for sid in ORDER + sorted(set(per) - set(ORDER)):
        if sid not in per and sid not in ref:
            continue
        d, rf = per.get(sid), ref.get(sid)
        exp = EXPECT.get(sid)
        rows.append({
            "id": sid, "name": NAMES.get(sid) or (d and d["name"]) or str(sid),
            "ref": rf and {"casts": rf["full"], "med": rf["med"], "min": rf["min"], "max": rf["max"],
                           "hitsPer": round(rf["hits"] / rf["full"], 2) if rf["full"] else None},
            "expect": exp[0] if exp else None, "expectSrc": exp[1] if exp else NO_HIT.get(sid, ""),
            "limit": rf["max"] + SLACK_MS if rf else None,
            "mine": d and {"casts": d["casts"], "ok": d["ok"], "cut": d["cut"],
                           "med": median(d["durs"]) if d["durs"] else None,
                           "long": d["long"], "few": d["few"], "nobuff": d["nobuff"],
                           "hits": dict(sorted(d["hits"].items()))},
        })
    return {"rows": rows, "logs": logs, "sword": stats(sw_all), "bow": stats(bow_all),
            "slack": SLACK_MS, "cut": CUT_MS}


def uptime(r, buff):
    """Share of the log from the first cast to the end with the buff on (apply 69 to all-gone 72)."""
    if not r["casts"]:
        return None
    a, b = r["casts"][0]["t"], r["end"]
    on, ev = 0, sorted([(t, 1) for t in r["buffs"].get(buff, [])] + [(t, 0) for t in r["buffs_gone"].get(buff, [])])
    since = None
    for t, kind in ev:
        if kind and since is None:
            since = t
        elif not kind and since is not None:
            on += max(0, min(t, b) - max(since, a))
            since = None
    if since is not None:
        on += max(0, b - max(since, a))
    return round(100 * on / (b - a)) if b > a else None


def reference_visits(cs):
    """Visit lengths in the reference log from its casts (build.py's list, with swaps as id -2)."""
    sws = [c["t"] for c in cs if c["id"] == -2]
    ids = [[c["id"] for c in cs if a <= c["t"] < b and c["id"] != -2]
           for a, b in zip(sws, sws[1:] + [float("inf")])]
    s, b = visits(sws, ids, None)
    return {"sword": stats(s), "bow": stats(b)}
