"""A new build for Bench Buddy, starting from the template: your golem pulls against a bench log.

Copy this folder to modules/<your build>/ (a name without a leading "_"), fill in the four settings
below, and restart run.bat. The checker then serves your page at http://127.0.0.1:8771/<ROUTE>.

What the checker asks of a module (the "simple contract", see AGENTS.md):
    NAME, ROUTE         shown on the page, and the address it is served at
    logs()              your golem logs of this build, oldest first
    compare(path)       one pull against the bench, as JSON for page.html
    history()           optional: one row per pull for the picker

This first version lines up your casts with the bench's in order (like a diff of two lists) and
gives each matched cast its start time against the bench's. Everything else (loops, cooldowns,
hits per cast, buffs) is yours to add; modules/berserker and modules/scrapper show how far it goes.
core/kit.py has the pieces they share (reading the bench, lining up casts, cached history, icons).
"""
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent.parent / "core"))
import evtc_rot  # noqa: E402  (core/evtc_rot.py: one player's casts, hits, swaps and buffs from a raw log)
import kit  # noqa: E402  (core/kit.py: helpers the modules share)
import web  # noqa: E402  (core/web.py: downloads from dps.report and the GW2 API)

# ---- the four settings to fill in ------------------------------------------------------------
NAME = "Power Scrapper (template)"
ROUTE = "template"            # the page's address: http://127.0.0.1:8771/template
ELITE = 43                    # the elite spec's id in arcdps: 18 Berserker, 43 Scrapper (see AGENTS.md)
SIGNATURE = (30088, 5807)     # skill ids only this build casts (Electro-whirl, Shrapnel Grenade)
PERMALINK = "55xd-20260422-192633_golem"   # the bench log on dps.report (the part after dps.report/)
# -----------------------------------------------------------------------------------------------

BENCH = HERE / "cache" / "bench.json"
CUT_MS = 100     # casts shorter than this are cancelled clicks, not casts
_bench = None


def logs():
    """Your logs of this build: Standard Kitty Golem logs where a player of ELITE cast a SIGNATURE skill."""
    return evtc_rot.golem_logs(elite=ELITE, needs=SIGNATURE)


def bench():
    """The bench's casts from its Elite Insights JSON, downloaded from dps.report the first time.
    Times are ms from its first cast."""
    global _bench
    if _bench is None:
        log = web.bench_log(PERMALINK, BENCH)
        pl, names = log["players"][0], log["skillMap"]
        casts = [c for c in kit.bench_casts(log, CUT_MS) if not c["cut"]]
        t0 = casts[0]["t"]
        for c in casts:
            c["t"] -= t0
        _bench = {"player": pl["name"], "casts": casts,
                  "icons": {k[1:]: v["icon"] for k, v in names.items() if v.get("icon")}}
    return _bench


def mine(path):
    """Your casts from the raw log, ms from your first cast."""
    r = evtc_rot.read(path)
    casts = [{"id": c["id"], "name": c["name"], "t": c["t"], "dur": c["dur"] or 0, "hits": c["all"], "cut": False}
             for c in r["casts"] if (c["dur"] or 0) >= CUT_MS]
    t0 = casts[0]["t"] if casts else 0
    for c in casts:
        c["t"] -= t0
    return r, casts


def compare(path):
    """Your casts lined up with the bench's: each row is a matched pair, a cast the bench has and you
    skipped ("missed") or one you added ("extra"). "behind" is how much later than the bench you
    started that cast (both counted from the first cast); the summary's "behind" is the median over
    the matched casts, since a long diff can pair a cast with a far-off bench cast now and then."""
    b = bench()
    r, yours = mine(path)
    end = yours[-1]["t"] + 2000 if yours else 0
    ref = [c for c in b["casts"] if c["t"] <= end]   # the bench runs longer than a short pull
    rows = []
    for it in kit.match(yours, ref, ordered=True):
        if "ghost" in it:
            rows.append({"kind": "missed", "bench": it["ghost"]})
        elif it["r"] is None:
            rows.append({"kind": "extra", "you": it["c"]})
        else:
            rows.append({"kind": "same", "bench": it["r"], "you": it["c"], "behind": it["c"]["t"] - it["r"]["t"]})
    rows = rows[:max(i for i, x in enumerate(rows) if "you" in x) + 1] if yours else []
    matched = [x for x in rows if x["kind"] == "same"]
    return {"log": Path(path).stem, "name": NAME, "player": r["player"], "bench": PERMALINK,
            "secs": r["end"] / 1000, "casts": len(yours),
            "missed": sum(x["kind"] == "missed" for x in rows), "extra": sum(x["kind"] == "extra" for x in rows),
            "behind": sorted(x["behind"] for x in matched)[len(matched) // 2] if matched else None, "rows": rows, "icons": b["icons"]}


if __name__ == "__main__":
    # python modules/<your build>/module.py   prints your newest pull, without the checker
    ls = logs()
    if not ls:
        sys.exit("no logs of this build yet: check ELITE and SIGNATURE")
    out = compare(ls[-1])
    print(out["log"], out["player"], f"{out['secs']:.1f} s", out["casts"], "casts,", out["missed"], "missed,", out["extra"], "extra")
    for x in out["rows"][:30]:
        c = x.get("you") or x["bench"]
        behind = "" if x.get("behind") is None else f"{x['behind'] / 1000:+.2f} s"
        print(f"  {x['kind']:<7} {c['name']:<28} {behind}")
