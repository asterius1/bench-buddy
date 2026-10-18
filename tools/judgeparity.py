"""Holds the Rust judging (judge/) against the Python checker on every Scrapper log.

    python tools/judgeparity.py            build judge/ and compare all your Scrapper pulls
    python tools/judgeparity.py --show 5   print up to 5 differences per pull (default 3)

For each pull it runs `compare()` and the addon panel in both and compares the JSON: the same keys,
the same texts and the same numbers (1e-6 apart at most; Python writes 5.0 where Rust writes 5).
The bench cheat sheet is compared too. Prints the timings of both. Exits with 1 when any differ.
"""
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "modules" / "scrapper"))
sys.path.insert(0, str(ROOT / "core"))
import panel  # noqa: E402
import scrap  # noqa: E402


def diff(a, b, path="", out=None):
    """Differences between two JSON values as (path, python, rust)."""
    out = [] if out is None else out
    if isinstance(a, bool) or isinstance(b, bool):
        if a is not b:
            out.append((path, a, b))
    elif isinstance(a, (int, float)) and isinstance(b, (int, float)):
        if abs(a - b) > 1e-6 * max(1, abs(a)):
            out.append((path, a, b))
    elif isinstance(a, dict) and isinstance(b, dict):
        for k in a.keys() | b.keys():
            if k not in a or k not in b:
                out.append((f"{path}.{k}", a.get(k, "<missing>"), b.get(k, "<missing>")))
            else:
                diff(a[k], b[k], f"{path}.{k}", out)
    elif isinstance(a, list) and isinstance(b, list):
        if len(a) != len(b):
            out.append((f"{path} length", len(a), len(b)))
        for i, (x, y) in enumerate(zip(a, b)):
            diff(x, y, f"{path}[{i}]", out)
    elif a != b:
        out.append((path, a, b))
    return out


def short(v):
    s = json.dumps(v, ensure_ascii=False)
    return s if len(s) < 120 else s[:117] + "..."


def main():
    show = int(sys.argv[sys.argv.index("--show") + 1]) if "--show" in sys.argv else 3
    judge = ROOT / "judge"
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=judge, check=True)
    exe = judge / "target" / "release" / ("bbjudge.exe" if sys.platform == "win32" else "bbjudge")
    with tempfile.TemporaryDirectory() as tmp:
        t = time.time()
        r = subprocess.run([str(exe), "dump", tmp], cwd=ROOT, capture_output=True, text=True)
        rust_s = time.time() - t
        if r.returncode:
            sys.exit(r.stderr)
        print("Rust:  ", r.stderr.strip(), f"(whole run {rust_s:.2f} s)")
        t = time.time()
        logs = scrap.logs()
        tl = time.time() - t
        t = time.time()
        ref = {p.stem: {"compare": scrap.compare(p)} for p in logs}
        for p in logs:
            ref[p.stem]["panel"] = panel.panel(p)
        tc = time.time() - t
        print(f"Python: {len(logs)} logs found in {tl:.2f} s; compare+panel all: {tc:.2f} s")
        bad = 0
        for p in logs:
            f = Path(tmp) / f"{p.stem}.json"
            if not f.exists():
                print(f"  {p.stem}: no Rust result")
                bad += 1
                continue
            d = diff(ref[p.stem], json.loads(f.read_text(encoding="utf-8")))
            if d:
                bad += 1
                print(f"  {p.stem}: {len(d)} differences")
                for path, a, b in d[:show]:
                    print(f"      {path}: python {short(a)}  rust {short(b)}")
        extra = {f.stem for f in Path(tmp).glob("*.json")} - set(ref) - {"_sheet"}
        for s in sorted(extra):
            print(f"  {s}: Rust found it, Python didn't")
            bad += 1
        d = diff(panel.sheet(), json.loads((Path(tmp) / "_sheet.json").read_text(encoding="utf-8")))
        for path, a, b in d[:show]:
            print(f"  sheet {path}: python {short(a)}  rust {short(b)}")
        print(f"{len(logs) - bad + len(extra)} of {len(logs)} pulls the same in both;",
              "the cheat sheet the same" if not d else f"the cheat sheet has {len(d)} differences")
        sys.exit(1 if bad or d else 0)


if __name__ == "__main__":
    main()
