"""The scrapper pull and the bench cheat sheet reduced to what the Nexus addon draws (`addon/src/scrap.rs`).

    panel(path)   the grid of `grid.html`: per cycle three whirl cells (bomb burst, nade filler, other
                  filler) and the cycle split, every icon and card already decided
    sheet()       the bench rotation of `bench.html`, as rows of icon tokens

The judging mirrors `grid.html` (pairing of moved fillers, the 20 s skill mark beyond the cycle's
drift, differences only): change both together. Texts are final; the addon only lays them out.
"""
import scrap

LATE_OK = 400
WHIRL = ["bomb burst", "nade filler", "other filler"]
HAMMER = scrap.HAMMER


def sgn(ms):
    if ms is None:
        return ""
    return ("+" if ms > 0 else "−" if ms < 0 else "±") + f"{abs(ms) / 1000:.2f}"


def sec(ms):
    return "—" if ms is None else f"{ms / 1000:.2f} s"


def clk(ms):
    if ms is None:
        return "—"
    neg, s = ms < 0, abs(ms) / 1000
    return ("−" if neg else "") + f"{int(s // 60)}:{s % 60:04.1f}"


def tone(ms):
    """0 neutral, 1 ahead (green), 2 behind (red), as the page's cls()."""
    if ms is None:
        return 0
    return 2 if ms > 50 else 1 if ms < -50 else 0


def _chips(mini):
    return [c for s in mini["sections"].values() for c in s["chips"]] if mini else []


def _judge(x):
    """Moved fillers and the 20 s marks, as grid.html's pairMoves()."""
    tw = set(scrap.TWENTY)
    for r in x["rows"]:
        cs = [c for m in r["minis"] for c in _chips(m)] + (r["pre"]["chips"] if r.get("pre") else [])
        lt = [c for c in cs if c["id"] in tw and c.get("late") is not None]
        base = min((c["late"] for c in lt), default=0)
        rl = [c for c in lt if c.get("refLate") is not None]
        rbase = min((c["refLate"] for c in rl), default=0)
        for c in lt:
            c["excess"] = c["late"] - base - ((c["refLate"] - rbase) if c.get("refLate") is not None else 0)
        ws = [(k, c) for k, m in enumerate(r["minis"]) for c in _chips(m)]
        ghosts = [(k, c) for k, c in ws if c["kind"] == "ghost"]
        for k, c in ws:
            if c["kind"] != "extra":
                continue
            g = next(((gk, gc) for gk, gc in ghosts if gc["id"] == c["id"] and gk != k and not gc.get("_used")), None)
            if g:
                g[1]["_used"] = True
                g[1]["kind"], g[1]["movedTo"] = "movedfrom", k
                c["kind"], c["movedFrom"] = "moved", g[0]


def _flags(c, fervor):
    f = [c["kind"]] if c["kind"] in ("ghost", "extra", "moved", "movedfrom") else []
    f += [x for x in c.get("flags", []) if x in ("slow", "few")]
    if c.get("excess") is not None and c["excess"] > LATE_OK:
        f.append("cdlate")
    if c.get("pre") == "hit":
        f.append("inferred")
    if c.get("t") is not None and any(a <= c["t"] < b for a, b in fervor):
        f.append("fervor")
    return f


def _problem(f):
    return any(x in f for x in ("ghost", "extra", "moved", "slow", "few", "cdlate"))


def _card(c, fervor):
    """[label, value, tone] rows of a skill's card; tone 0 plain, 1 green, 2 red, 3 amber, 4 muted, 5 blue."""
    rows = []
    k = c["kind"]
    if k == "ghost":
        rows.append(["", "the bench casts it here, you didn't", 2])
    if k == "movedfrom":
        rows.append(["", f"you cast it in the {WHIRL[c['movedTo']]} instead", 4])
    if k == "moved":
        rows.append(["", f"the bench casts it in the {WHIRL[c['movedFrom']]}", 5])
    if k == "extra":
        rows.append(["", "the bench has no such cast in this cycle", 3])
    if k in ("ghost", "movedfrom"):
        rows += [["Bench cast time", sec(c.get("refDur")), 0], ["Bench at", clk(c.get("refT")), 0]]
        return rows
    ref = c.get("ref")
    rows.append(["At", clk(c.get("t")) + (f"   bench {clk(ref['t'])}" if ref else ""), 0])
    if c.get("pre"):
        rows.append(["", {"end": "under way at the fight start: placed from its end",
                          "order": "mine placed before the fight: time from your opener order"}.get(
            c["pre"], f"placed before the fight: from its first hit at {clk(c.get('hit'))}"), 4])
    rows.append(["Cast time", sec(c.get("dur")) + (f"   bench {sec(c['refDur'])}" if c.get("refDur") is not None else ""), 0])
    if c.get("landed") is not None:
        rows.append(["Hits", f"{c['landed']} of {c['expect']}", 2 if c["landed"] < c["expect"] else 0])
    if c.get("stop") and c["stop"] != "done":
        rows.append(["Ended by", c["stop"], 0])
    if c.get("slot") is not None and not c.get("pre"):
        rows.append(["Start to next cast", sec(c["slot"]) + (f"   bench {sec(c['refSlot'])}" if c.get("refSlot") is not None else ""), 0])
    if c.get("lost") is not None and not c.get("pre"):
        rows.append(["Lost here", sgn(c["lost"]), tone(c["lost"])])
    if c.get("cdWait"):
        rows.append(["Next skill on cooldown", f"{sec(c['cdWait'])} (not counted)", 4])
    if c.get("late") is not None and c["late"] >= 0:
        v = sgn(c["late"]) + (f"   bench {sgn(c['refLate'])}" if c.get("refLate") is not None else "")
        if c.get("excess") is not None and c["excess"] > 50:
            v += f"   {sgn(c['excess'])} beyond the cycle's drift"
        rows.append(["After ready", v, 3 if c.get("excess", 0) > LATE_OK else 0])
    if c.get("t") is not None and any(a <= c["t"] < b for a, b in fervor):
        rows.append(["Bloodstone Fervor", "on (+7% strike)", 0])
    return rows


def _icon(c, fervor):
    return {"id": c["id"], "name": c["name"], "f": _flags(c, fervor), "card": _card(c, fervor)}


def _tokens(chips, fervor):
    """All casts of a cell compressed like grid.html: Electro-whirl + Shrapnel one token, autos one."""
    out = []
    for c in chips:
        if c["kind"] == "cut":
            continue
        last = out[-1] if out else None
        real = c["kind"] not in ("ghost", "movedfrom")
        if real and c["id"] == scrap.SHRAP and last and last["t"] == "ew" and len(last["casts"]) == 1:
            last["casts"].append(_icon(c, fervor))
            continue
        if real and c["id"] in HAMMER:
            if last and last["t"] == "aa":
                last["casts"].append(_icon(c, fervor))
                continue
            out.append({"t": "aa", "casts": [_icon(c, fervor)]})
            continue
        if real and c["id"] == scrap.EW:
            out.append({"t": "ew", "casts": [_icon(c, fervor)]})
            continue
        out.append({"t": "one", "casts": [_icon(c, fervor)]})
    ghost = [t for t in out if any(x in t["casts"][0]["f"] for x in ("ghost", "movedfrom"))]
    return [t for t in out if t not in ghost] + ghost


def _tiles(chips):
    out = []
    for c in chips:
        if c["kind"] == "cut":
            continue
        gone = c["kind"] in ("ghost", "movedfrom")
        b = c["id"] if gone else (c["ref"]["id"] if c.get("ref") else None)
        y = None if gone else c["id"]
        f = c.get("flags", [])
        if "few" in f:
            lab, t = f"{c['landed']}/{c['expect']}", 2
        elif c["kind"] == "ghost":
            lab, t = "miss", 2
        elif c["kind"] == "movedfrom":
            lab, t = f"to {WHIRL[c['movedTo']].split()[0]}", 4
        elif c["kind"] == "moved":
            lab, t = f"from {WHIRL[c['movedFrom']].split()[0]}", 5
        elif c["kind"] == "extra":
            lab, t = "extra", 3
        else:
            lab, t = sgn(c.get("lost")), tone(c.get("lost"))
        # times for the card's two lanes: the bench's cast and yours, ms from each side's cycle start
        ref = c.get("ref") or {}
        bt, bd = (c.get("refT"), c.get("refDur")) if gone else (ref.get("t"), ref.get("dur"))
        out.append({"b": b, "y": y, "lab": lab, "tone": t, "name": c["name"],
                    "bt": bt, "bd": bd, "yt": None if gone else c.get("t"), "yd": None if gone else c.get("dur"),
                    "slow": "slow" in f, "few": "few" in f, "kind": c["kind"]})
    return out


def _cell(r, k, x, fervor):
    m = r["minis"][k] if k < len(r["minis"]) else None
    if m is None and k == 0 and r.get("pre"):
        chips = r["pre"]["chips"]
        first = next((mm for mm in r["minis"] if mm), None)
        d = first["start"] - first["refStart"] if first and first.get("refStart") is not None else None
        title, sub, time = "Opener", "From the fight start. Pre-fight casts: from their first hits (dotted), mines in your order.", first["start"] if first else None
        splits = []
    elif m is None:
        return None
    else:
        chips = [c for c in _chips(m) if m["len"] is not None or c["kind"] != "ghost"]
        d = m["len"] - m["refLen"] if m["len"] is not None and m.get("refLen") is not None else None
        time = m["len"]
        title = f"Cycle {r['n']} · {WHIRL[k].capitalize()}" + (f" (bench cycle {r['refN']})" if r.get("refN") not in (None, r["n"]) else "")
        sub = f"{sec(m['len'])} · bench {sec(m.get('refLen'))} · {sgn(d)} · starts {clk(m['start'])}, bench {clk(m.get('refStart'))}"
        names = {"core": "Electro-whirl + Shrapnel", "fill": "Filler", "burst": "Bomb burst"}
        splits = [[names[n], sec(s["time"]), sec(s["refTime"]),
                   sgn(s["time"] - s["refTime"]) if s["time"] is not None and s["refTime"] is not None else "",
                   tone(s["time"] - s["refTime"]) if s["time"] is not None and s["refTime"] is not None else 0]
                  for n, s in m["sections"].items()]
    icons = [_icon(c, fervor) for c in chips if c["kind"] != "cut"]
    return {"label": "opener" if m is None else clk(m["start"]), "time": time, "delta": d,
            "border": "err" if any(c["kind"] == "ghost" for c in chips) else "xtra" if any(c["kind"] == "extra" for c in chips) else "",
            "diff": [i for i in icons if _problem(i["f"])], "all": _tokens(chips, fervor),
            "card": {"title": title, "sub": sub, "splits": splits, "tiles": _tiles(chips),
                     "len": time if m else None, "refLen": m.get("refLen") if m else None}}


def _cyc_card(r):
    rows = [[c["name"], clk(c["t"]), sgn(c["late"]) if c.get("late") is not None else "—",
             sgn(c["refLate"]) if c.get("refLate") is not None else "—"] for c in r["cd"]]
    rows += [[x["name"], "bench casts it, you didn't", "", ""] for x in r["cdMissing"]]
    fm, fr, b, rb = r["fervor"]["mine"], r["fervor"]["ref"], r["buffs"], r.get("refBuffs") or {}
    foot = (f"Fervor covers {len(fm['inside'])} of {fm['of']} 20 s skills"
            + (f" (bench {len(fr['inside'])} of {fr['of']})" if fr else "")
            + f" · Superspeed {b['superspeed']}% (bench {rb.get('superspeed', '—')}%)")
    return {"title": f"Cycle {r['n']}", "sub": f"{sec(r['len'])} · bench {sec(r['refLen'])}. A 20 s skill is ready 16 s after its cast ends with alacrity.",
            "rows": rows, "foot": foot}


def panel(path):
    x = scrap.compare(path)
    if not x:
        return None
    _judge(x)
    fervor = x["fervorIv"]
    rows, behind = [], 0
    for r in x["rows"]:
        d = r["len"] - r["refLen"] if r["len"] is not None and r.get("refLen") is not None else None
        if d is not None:
            behind += d
        rows.append({"n": r["n"], "start": clk(r["start"]),
                     "late": f"+{r['late20'] / 1000:.1f}" if r.get("late20") is not None else "",
                     "lateTone": tone(r["late20"] - r["refLate20"]) if r.get("late20") is not None and r.get("refLate20") is not None else 0,
                     "len": r["len"], "delta": d, "behind": behind if d is not None else None,
                     "cells": [_cell(r, k, x, fervor) for k in range(3)], "card": _cyc_card(r)})
    B, R = x["buffs"], x["refBuffs"]
    last = x["lag"][-1] if x["lag"] else None
    return {"kind": "scrapper", "log": x["log"], "secs": x["secs"], "benchSecs": x["benchSecs"],
            "head": [["behind", sgn(last), tone(last)],
                     ["DPS", f"{x['dps'] / 1000:.1f}k {x['dps'] / x['benchDps'] * 100 - 100:+.0f}%", 2 if x["dps"] < x["benchDps"] else 1],
                     ["Fervor", f"{B['fervor']}%  {R['fervor']}", 0],
                     ["SS", f"{B['superspeed']}%  {R['superspeed']}", 0],
                     ["Motion", f"+{B['motion']['bonus']:.1f}%  {R['motion']['bonus']:.1f}", 0]],
            "rows": rows}


def sheet():
    """The bench rotation for the addon's cheat sheet view: the structure on top (opener and the fixed
    part of each mini loop, with a slot for the filler), then rows of three cells of just the filler."""
    s = scrap.bench_sheet()
    tok = lambda t: {"t": t["kind"], "id": t["id"], "name": t["name"], "n": t.get("n", 1),
                     "kit": t.get("kit", ""), "usual": t.get("usual", True),
                     "marks": [k2["id"] for k2 in t.get("marks", [])], "card": _sheet_card(t)}
    rows = []
    for r in s["rows"]:
        cells = []
        for k in range(3):
            m = r["minis"][k] if k < len(r["minis"]) else None
            if m is None and k == 0 and r["pre"]:
                cells.append({"label": "opener", "len": None, "tokens": [], "missing": []})
                continue
            if m is None:
                cells.append(None)
                continue
            cells.append({"label": f"+{m['start'] / 1000:.1f} s", "len": m["len"],
                          "tokens": [tok(t) for t in m["filler"]],
                          "missing": [{"id": y["id"], "name": y["name"], "kit": y.get("kit", "")} for y in m["missing"]]})
        rows.append({"n": r["n"], "start": clk(r["start"]), "len": r["len"], "cells": cells})
    return {"kind": "sheet", "rows": rows,
            "structure": [{"name": c["name"], "tokens": [tok(t) for t in c["tokens"]]} for c in s["structure"]],
            "usual": [[u["id"] for u in col if u["id"] not in scrap.SHOWN_INSTANT] for col in s["usual"]]}


def _sheet_card(t):
    if t["kind"] == "slot":
        return [["", "the floating skills of each cycle go here (see the rows below)", 0]]
    rows = [["At", clk(t["t"]), 0]]
    if t["kind"] == "aa":
        rows = [["Auto casts", str(t["n"]), 0], ["From", clk(t["t"]), 0], ["Length", sec(t["end"] - t["t"]), 0]]
    elif t["kind"] == "ew":
        rows = [["Electro-whirl", clk(t["t"]), 0], ["Shrapnel", clk(t.get("shrap")), 0]]
    else:
        if t.get("dur"):
            rows.append(["Cast time", sec(t["dur"]), 0])
        if t.get("cd"):
            rows.append(["Recharge", f"{t['cd']} s, {t['cd'] / 1.25:.1f} s with alacrity", 0])
        if t.get("late") is not None:
            rows.append(["Cast after ready", sec(t["late"]), 0])
        if t.get("usual") is False:
            rows.append(["", "floating: not in this whirl every cycle", 5])
    rows += [[k["name"], clk(k["t"]), 4] for k in t.get("marks", [])]
    return rows


if __name__ == "__main__":
    import json
    p = scrap.logs()[-1]
    out = panel(p)
    print(json.dumps(out)[:1500])
    print(len(json.dumps(out)), "bytes;", len(json.dumps(sheet())), "bytes sheet")
