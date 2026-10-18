"""Holds the Rust checker against the Python one, route by route, on every pull.

    python tools/parity.py                build the Rust checker and compare everything
    python tools/parity.py --show 5       print up to 5 differences per answer (default 3)

Starts the Python checker (core/serve.py on 8791) and live judge (core/live_server.py on 8792) and the
Rust checker (target/release/checker on 8793, with its own empty cache so it downloads and computes
everything itself), then asks all three the same questions and compares the JSON: the same keys, the
same texts and the same numbers (1e-6 apart at most; Python writes 5.0 where Rust writes 5).

  routes   every JSON route of both builds; compare and pull for every Berserker pull, compare and
           panel for every Scrapper pull; the pages and the Berserker views
  live     every saved live capture (<log>.live.json next to the logs) judged by both live judges

Exits with 1 when anything differs.
"""
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "core"))
import evtc_rot  # noqa: E402

PY, LIVE, RS = 8791, 8792, 8793


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


INSTANT = {6162, 6166, -37, 76326}   # the Scrapper sheet's instants


def normal(path, v):
    """What may differ on purpose: the Scrapper bench sheet's usual skills that are instants all tie
    at the end of their column, in Python's set order; compared as a set."""
    if path.startswith("/api/scrapper/bench") and isinstance(v, dict):
        for col in v.get("usual", []):
            col[:] = [x for x in col if x["id"] not in INSTANT] + sorted((x for x in col if x["id"] in INSTANT), key=lambda x: x["id"])
    return v


def view_parts(html):
    """A Berserker view: the page around the inlined data, and the data (PULL, ICONS) as JSON."""
    a = html.find("<script>window.PULL = ")
    b = html.find("</script>", a)
    if a < 0 or b < 0:
        return html, None
    data = html[a + len("<script>window.PULL = "):b].replace(r"<\/", "</")
    pull, _, icons = data.partition(";\nwindow.ICONS = ")
    return html[:a] + html[b:], [json.loads(pull), json.loads(icons.rstrip().rstrip(";"))]


def short(v):
    s = json.dumps(v, ensure_ascii=False)
    return s if len(s) < 120 else s[:117] + "..."


def get(port, path, body=None):
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=body,
                                 headers={"Content-Type": "application/json"} if body else {})
    try:
        with urllib.request.urlopen(req, timeout=300) as r:
            return r.read().decode("utf-8")
    except urllib.error.HTTPError as e:
        return f"<{e.code}: {e.read().decode('utf-8', 'replace')[:200]}>"


def wait(port):
    for _ in range(600):
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{port}/api/version" if port != LIVE else f"http://127.0.0.1:{port}/", timeout=2)
            return
        except Exception:  # noqa: BLE001
            time.sleep(0.2)
    sys.exit(f"nothing answers on {port}")


def main():
    show = int(sys.argv[sys.argv.index("--show") + 1]) if "--show" in sys.argv else 3
    subprocess.run(["cargo", "build", "--release", "--quiet", "-p", "checker"], cwd=ROOT, check=True)
    exe = ROOT / "target" / "release" / ("checker.exe" if sys.platform == "win32" else "checker")
    cache = Path(tempfile.mkdtemp(prefix="bb-parity-"))
    procs = [subprocess.Popen([sys.executable, "core/serve.py", str(PY), "--no-browser"], cwd=ROOT, stdout=subprocess.DEVNULL),
             subprocess.Popen([sys.executable, "core/live_server.py", str(LIVE)], cwd=ROOT, stdout=subprocess.DEVNULL),
             subprocess.Popen([str(exe), str(RS), "--no-browser"], cwd=ROOT, stdout=subprocess.DEVNULL,
                              env={**os.environ, "BENCH_BUDDY_CACHE": str(cache)})]
    bad = 0
    try:
        for p in (PY, LIVE, RS):
            wait(p)
        timing = {PY: 0.0, RS: 0.0}

        def both(path, html=False):
            nonlocal bad
            out = []
            for port in (PY, RS):
                t = time.time()
                out.append(get(port, path))
                timing[port] += time.time() - t
            a, b = out
            if html:
                (pa, da), (pb, db) = view_parts(a), view_parts(b)
                d = ([] if pa == pb else [("page", pa[:80], pb[:80])]) + (diff(da, db) if da is not None or db is not None else [])
            else:
                try:
                    d = diff(normal(path, json.loads(a)), normal(path, json.loads(b)))
                except ValueError:
                    d = [("", a[:100], b[:100])] if a != b else []
            if d:
                bad += 1
                print(f"  {path}: {len(d)} differences")
                for p, x, y in d[:show]:
                    print(f"      {p}: python {short(x)}  rust {short(y)}")
            return not d

        fixed = ["/api/latest", "/api/history", "/api/icons", "/api/modules", "/api/scrapper/latest",
                 "/api/scrapper/history", "/api/scrapper/sheet", "/api/scrapper/bench", "/api/scrapper/icons"]
        same = sum(both(p) for p in fixed)
        print(f"routes: {same} of {len(fixed)} the same")
        bers = [p.stem for p in evtc_rot.golem_logs()]
        scr = [p.stem for p in evtc_rot.golem_logs(elite=43, needs=(30088, 5807))]
        n = sum(both(f"/api/compare?log={s}") and both(f"/api/pull?log={s}") for s in bers)
        print(f"Berserker: {n} of {len(bers)} pulls the same (compare and pull)")
        n = sum(both(f"/api/scrapper/compare?log={s}") and both(f"/api/scrapper/panel?log={s}") for s in scr)
        print(f"Scrapper: {n} of {len(scr)} pulls the same (compare and panel)")
        pages = ["/berserker", "/scrapper", "/scrapper/bench", "/scrapper/splits"] + \
                ([f"/view/{v}?log={bers[-1]}" for v in ("splits", "grid", "timeline", "coach")] if bers else [])
        n = sum(both(p, html=True) for p in pages)
        print(f"pages: {n} of {len(pages)} the same")
        print(f"time: python {timing[PY]:.1f} s, rust {timing[RS]:.1f} s")
        caps = sorted(evtc_rot.GOLEM.glob("*.live.json"))
        n = 0
        for c in caps:
            cap = json.loads(c.read_text(encoding="utf-8"))
            body = json.dumps({"elite": cap.get("elite"), "events": cap.get("events", [])}).encode()
            a, b = get(LIVE, "/compare", body), get(RS, "/api/live", body)
            d = diff(json.loads(a), json.loads(b)) if a.startswith("{") and b.startswith("{") else ([("", a[:100], b[:100])] if a != b else [])
            if d:
                bad += 1
                print(f"  live {c.name}: {len(d)} differences")
                for p, x, y in d[:show]:
                    print(f"      {p}: python {short(x)}  rust {short(y)}")
            else:
                n += 1
        print(f"live: {n} of {len(caps)} captures judged the same")
    finally:
        for p in procs:
            p.kill()
        shutil.rmtree(cache, ignore_errors=True)
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
