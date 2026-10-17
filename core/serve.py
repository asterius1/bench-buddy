"""The Bench Buddy checker: http://127.0.0.1:8771/ (Condi Berserker) and /scrapper (Power Scrapper)

    python core/serve.py [port] [--no-browser]       (or double-click run.bat)

Watches the Standard Kitty Golem log folder: each page asks every few seconds for the newest log of
its build and shows it as soon as arcdps has written it. Nothing is uploaded; each module's bench
log is downloaded once from dps.report into its cache/ folder. Any other modules/<folder>/module.py
(the simple contract in AGENTS.md) is served at /<its ROUTE>.

Picks up changes to its own code without a restart: the modules in MODULES are
reloaded when they change, the pages are read on every request, and the open page reloads itself
when any of them changes. A change to serve.py itself ends the process with exit code 3, which
run.bat takes as "start me again".
"""
import importlib
import importlib.util
import json
import sys
import threading
import time
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlparse

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
BERS = ROOT / "modules" / "berserker"
SCRAP = ROOT / "modules" / "scrapper"
for d in (HERE, BERS, SCRAP):
    sys.path.insert(0, str(d))
import baseline  # noqa: E402
import evtc_rot  # noqa: E402
import local  # noqa: E402  (the Condi Berserker checker at /)

import scrap  # noqa: E402  (the Power Scrapper checker at /scrapper)
import panel as scrap_panel  # noqa: E402  (modules/scrapper/panel.py: what the addon draws)


def load_simple():
    """Modules that follow the simple contract (a `module.py` in modules/<folder>/, see AGENTS.md),
    each served at /<ROUTE>. Folders starting with "_" (the template) are skipped; a module that
    fails to load is reported and left out, so it cannot take the others down."""
    out = []
    for f in sorted((ROOT / "modules").glob("*/module.py")):
        if f.parent.name.startswith("_"):
            continue
        name = f"bb_{f.parent.name}"
        try:
            sys.path.insert(0, str(f.parent))
            spec = importlib.util.spec_from_file_location(name, f)
            m = importlib.util.module_from_spec(spec)
            sys.modules[name] = m
            spec.loader.exec_module(m)
            out.append(m)
        except Exception as e:  # noqa: BLE001
            sys.modules.pop(name, None)
            print(f"module {f.parent.name} not loaded: {e!r}", flush=True)
    return out


SIMPLE = load_simple()
SIMPLE_BY_ROUTE = {m.ROUTE: m for m in SIMPLE}

PORT = 8771
_lock = threading.Lock()
_pulls = {}      # stem -> (size, pull dict)
_seen = {"files": None, "logs": []}
STARTED = int(time.time())   # a restart also makes the open page reload
RESTART = 3          # exit code that run.bat answers by starting the server again
MODULES = [evtc_rot, baseline, local, scrap, scrap_panel] + SIMPLE   # reloaded in this order


def watched():
    """The code and page files whose changes the server and the open page pick up."""
    pages = [str(BERS / "page.html")] + sorted(map(str, SCRAP.glob("*.html"))) + sorted(map(str, (BERS / "views").glob("*.html")))
    for m in SIMPLE:
        pages += sorted(map(str, Path(m.__file__).parent.glob("*.html")))
    return [m.__file__ for m in MODULES] + pages


def stamp(files):
    out = {}
    for f in files:
        try:
            out[f] = Path(f).stat().st_mtime
        except OSError:
            pass
    return out


_version = {"stamp": stamp(watched()), "n": 0, "self": Path(__file__).stat().st_mtime}


def reload_if_changed(srv):
    """Runs in a thread: reload changed modules, bump the version the page watches,
    and stop with RESTART when serve.py itself changed."""
    while True:
        time.sleep(1)
        try:
            if Path(__file__).stat().st_mtime != _version["self"]:
                print("serve.py changed: restarting", flush=True)
                _version["exit"] = RESTART
                srv.shutdown()
                return
            now = stamp(watched())
            if now == _version["stamp"]:
                continue
            changed = [Path(f).name for f in now if now[f] != _version["stamp"].get(f)]
            with _lock:
                if any(n.endswith(".py") for n in changed):
                    for m in MODULES:
                        if m in SIMPLE:   # loaded from its file, not found by name: run the file again
                            m.__spec__.loader.exec_module(m)
                        else:
                            importlib.reload(m)
                    _pulls.clear()
                    _seen["files"] = None
                    for f in list(local.CACHE.glob("*.json")) + list(scrap.CACHE.glob("*.json")):
                        f.unlink()   # history rows may change with the code
                _version["stamp"] = now
                _version["n"] += 1
            print("reloaded:", ", ".join(changed), flush=True)
        except Exception as e:  # noqa: BLE001  (a half-saved file: keep the old code, try again)
            print("reload failed, keeping the old code:", e, flush=True)
            time.sleep(1)


def logs():
    """Berserker logs, rechecked when the folder's file list changes."""
    files = tuple(sorted((p.name, p.stat().st_size) for p in evtc_rot.GOLEM.glob("*.zevtc")
                         if p.stem >= evtc_rot.FIRST_ANIMATED))
    if files != _seen["files"]:
        try:
            _seen["logs"] = evtc_rot.golem_logs()
            _seen["files"] = files
        except Exception:  # a log still being written
            pass
    return _seen["logs"]


_scrap = {"files": None, "logs": []}


def scrap_logs():
    """Scrapper logs, rechecked when the folder's file list changes."""
    files = tuple(sorted((p.name, p.stat().st_size) for p in evtc_rot.GOLEM.glob("*.zevtc")
                         if p.stem >= evtc_rot.FIRST_ANIMATED))
    if files != _scrap["files"]:
        try:
            _scrap["logs"] = scrap.logs()
            _scrap["files"] = files
        except Exception:  # a log still being written
            pass
    return _scrap["logs"]


_simple = {}   # route -> {"files", "logs"}


def simple_logs(m):
    """A simple module's logs, rechecked when the folder's file list changes."""
    seen = _simple.setdefault(m.ROUTE, {"files": None, "logs": []})
    files = tuple(sorted((p.name, p.stat().st_size) for p in evtc_rot.GOLEM.glob("*.zevtc")
                         if p.stem >= evtc_rot.FIRST_ANIMATED))
    if files != seen["files"]:
        try:
            seen["logs"] = m.logs()
            seen["files"] = files
        except Exception:  # a log still being written
            pass
    return seen["logs"]


def simple_get(m, path, q):
    """The simple contract's routes: /<route>, /api/<route>/latest, /history, /compare?log=."""
    if path == f"/{m.ROUTE}":
        return (Path(m.__file__).parent / getattr(m, "PAGE", "page.html")).read_text(encoding="utf-8"), "text/html"
    if path == f"/api/{m.ROUTE}/latest":
        ls = simple_logs(m)
        return json.dumps({"latest": ls[-1].stem if ls else None, "count": len(ls), "name": m.NAME}), "application/json"
    if path == f"/api/{m.ROUTE}/history":
        ls = simple_logs(m)
        rows = m.history() if hasattr(m, "history") else [{"log": p.stem} for p in ls]
        return json.dumps(rows), "application/json"
    if path == f"/api/{m.ROUTE}/compare":
        return json.dumps(get_pull(q["log"][0], m.compare)), "application/json"
    return None


def get_pull(stem, fn=None):
    fn = fn or local.pull
    p = evtc_rot.GOLEM / f"{stem}.zevtc"
    size = p.stat().st_size
    key = (stem, fn.__module__, fn.__name__)
    hit = _pulls.get(key)
    if hit and hit[0] == size:
        return hit[1]
    x = fn(p)
    _pulls[key] = (size, x)
    return x


VIEWS = ("splits", "grid", "timeline", "coach")
# Hide each mockup's fake game backdrop: here the page is a plain window next to the game.
EMBED_CSS = """<style>
.game, .stage > .floor, .stage > .skillbar, .stage > .char, .stage > .golem,
#screen > .ground, #screen > .golem, #screen > .char, #screen > .bar, #stage > .fake, #stage > .fake2 { display: none !important; }
html, body, .stage, #screen, #stage { background: #0d0d0d !important; }
</style>"""


def view_page(name, stem):
    """One of the Berserker views (modules/berserker/views/<name>.html) with this pull's data inlined."""
    html = (BERS / "views" / f"{name}.html").read_text(encoding="utf-8")
    data = ("window.PULL = " + json.dumps(get_pull(stem, local.compare)) + ";\nwindow.ICONS = "
            + json.dumps(local.icons()) + ";")
    html = html.replace('<script src="data.js"></script>', "<script>" + data.replace("</", "<\\/") + "</script>")
    return html.replace("</head>", EMBED_CSS + "</head>", 1)


class Handler(BaseHTTPRequestHandler):
    def send(self, body, ctype="application/json"):
        b = body.encode("utf-8") if isinstance(body, str) else body
        self.send_response(200)
        self.send_header("Content-Type", ctype + "; charset=utf-8")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        u = urlparse(self.path)
        q = parse_qs(u.query)
        try:
            with _lock:
                if u.path == "/":
                    return self.send((BERS / "page.html").read_text(encoding="utf-8"), "text/html")
                if u.path == "/api/root":   # the addon starts core/live_server.py from here
                    return self.send(json.dumps({"root": str(ROOT)}))
                if u.path == "/api/version":
                    return self.send(json.dumps({"v": f"{STARTED}-{_version['n']}"}))
                if u.path == "/api/latest":
                    ls = logs()
                    return self.send(json.dumps({"latest": ls[-1].stem if ls else None, "count": len(ls)}))
                if u.path == "/api/compare":
                    return self.send(json.dumps(get_pull(q["log"][0], local.compare)))
                if u.path == "/api/pull":
                    return self.send(json.dumps(get_pull(q["log"][0])))
                if u.path == "/api/history":
                    logs()
                    return self.send(json.dumps(local.history()))
                if u.path.startswith("/view/"):
                    name = u.path[len("/view/"):]
                    if name in VIEWS:
                        return self.send(view_page(name, q["log"][0]), "text/html")
                if u.path == "/scrapper":
                    return self.send((SCRAP / "grid.html").read_text(encoding="utf-8"), "text/html")
                if u.path == "/api/scrapper/latest":
                    ls = scrap_logs()
                    return self.send(json.dumps({"latest": ls[-1].stem if ls else None, "count": len(ls)}))
                if u.path == "/api/scrapper/history":
                    scrap_logs()
                    return self.send(json.dumps(scrap.history()))
                if u.path == "/api/scrapper/compare":
                    return self.send(json.dumps(get_pull(q["log"][0], scrap.compare)))
                if u.path.startswith("/scrapper/") and u.path[10:].isalnum() and (SCRAP / f"{u.path[10:]}.html").exists():
                    return self.send((SCRAP / f"{u.path[10:]}.html").read_text(encoding="utf-8"), "text/html")
                if u.path == "/api/scrapper/panel":
                    return self.send(json.dumps(get_pull(q["log"][0], scrap_panel.panel)))
                if u.path == "/api/scrapper/sheet":
                    return self.send(json.dumps(scrap_panel.sheet()))
                if u.path == "/api/scrapper/bench":
                    return self.send(json.dumps(scrap.bench_sheet()))
                if u.path == "/api/scrapper/icons":
                    return self.send(json.dumps(scrap.icons()))
                if u.path == "/api/icons":
                    return self.send(json.dumps(local.icons()))
                if u.path == "/api/modules":
                    return self.send(json.dumps([{"name": "Condi Berserker", "route": ""}, {"name": "Power Scrapper", "route": "scrapper"}]
                                                + [{"name": m.NAME, "route": m.ROUTE} for m in SIMPLE]))
                for m in SIMPLE:
                    hit = simple_get(m, u.path, q)
                    if hit:
                        return self.send(*hit)
        except Exception as e:  # noqa: BLE001
            self.send_response(500)
            self.end_headers()
            self.wfile.write(str(e).encode())
            return
        self.send_response(404)
        self.end_headers()

    def log_message(self, *a):
        pass


def main():
    nums = [a for a in sys.argv[1:] if a.isdigit()]
    port = int(nums[0]) if nums else PORT
    srv = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    url = f"http://127.0.0.1:{port}/"
    print(f"Bench Buddy: Berserker on {url}, Scrapper on {url}scrapper (Ctrl+C to stop; code changes are picked up by themselves)", flush=True)
    for m in SIMPLE:
        print(f"  {m.NAME} on {url}{m.ROUTE}", flush=True)
    if "--no-browser" not in sys.argv:
        webbrowser.open(url)
    threading.Thread(target=reload_if_changed, args=(srv,), daemon=True).start()
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        return 0
    srv.server_close()
    return _version.get("exit", 0)


if __name__ == "__main__":
    sys.exit(main())
