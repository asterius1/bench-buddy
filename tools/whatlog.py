"""What your newest golem logs hold: the settings a new module needs (ELITE and SIGNATURE).

    python tools/whatlog.py [count]        (default the newest 5 logs)

For each log: your character, its elite spec id (ELITE) and your most cast skills with their ids,
so you can pick SIGNATURE: one or two skill ids that only your build casts.
"""
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent / "core"))
import evtc_rot  # noqa: E402

n = int(sys.argv[1]) if len(sys.argv) > 1 else 5
paths = sorted(evtc_rot.GOLEM.glob("*.zevtc"))[-n:]
if not paths:
    sys.exit(f"no golem logs in {evtc_rot.GOLEM} (set BENCH_BUDDY_LOGS if arcdps writes them elsewhere)")
for p in paths:
    try:
        r = evtc_rot.read(p)
    except Exception as e:  # noqa: BLE001  (a log still being written)
        print(p.stem, "unreadable:", e)
        continue
    top = Counter((c["id"], c["name"]) for c in r["casts"] if (c["dur"] or 0) >= 100).most_common(10)
    print(f"{p.stem}  {r['player']}  ELITE = {r['elite']}  ({r['end'] / 1000:.0f} s, {len(r['casts'])} casts)")
    for (sid, name), k in top:
        print(f"    {sid:>6}  {name:<30} x{k}")
    if not r["casts"]:
        print("    no casts: logs before October 2026 carry no animation events")
