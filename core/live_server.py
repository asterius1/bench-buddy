"""Judges a pull while it runs, with the checker's own code, for the in-game addon.

    pythonw core/live_server.py [port]      (default 8772; the addon starts it when it is not running)

The addon POSTs to /compare your events of the live arcdps feed and your elite spec:
    {"elite": 43, "events": [{"time", "sc", "buff", "skill", "value", "buff_dmg", "result", "dst_self"}]}
core/live.py turns them into the record `evtc_rot.read()` gives for a log, and the build's module
judges it as it judges a log: `evtc_rot.read` answers with the live record while the module runs.
So any module that reads its pulls through evtc_rot judges live pulls too, with no code of its own.
A module may adjust the record first in `live(record)` (the Berserker puts back its weapon swaps).

With "Save live captures" on, the addon POSTs to /capture, when a pull's log comes in, the events
it last sent for that pull and what it drew of them: {"log": stem, "elite", "events", "shown"}. They
are saved next to the log as `<stem>.live.json` (core/live.py `capture_of`) for tools/livetest.py.

What comes back is what the addon draws for a log of that build: {"kind": "berserker", "data":
/api/compare} or {"kind": "scrapper", "data": /api/scrapper/panel}; another module's compare()
as {"kind": its ROUTE}. Unknown elite: {"kind": null}.
The modules are reloaded when they change, as core/serve.py does. The server exits after 15
minutes without a request. A request that fails is logged with its traceback to
core/cache/live_server.log, and its body kept in core/cache/live_fail.json to replay it.
"""
import importlib
import json
import traceback
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import evtc_rot  # noqa: E402
import live  # noqa: E402
import serve  # noqa: E402  (loads every module as the checker does: local, scrap, panel, the simple ones)

MODULES = [evtc_rot, live] + [m for m in serve.MODULES if m is not evtc_rot]   # reloaded in this order
IDLE_EXIT_S = 15 * 60
CACHE = HERE / "cache"
_lock = threading.Lock()
_last = [time.time()]
_mtimes = {}


def reload_if_changed():
    changed = [m for m in MODULES if Path(m.__file__).stat().st_mtime != _mtimes.get(m.__name__)]
    if not changed:
        return
    if _mtimes:
        for m in MODULES:
            if m in serve.SIMPLE:
                m.__spec__.loader.exec_module(m)
            else:
                importlib.reload(m)
    for m in MODULES:
        _mtimes[m.__name__] = Path(m.__file__).stat().st_mtime


def judge_for(elite):
    """(kind, module, function) that judges a pull of this elite spec, or None."""
    if elite == 18:
        return "berserker", serve.local, serve.local.compare
    if elite == 43:
        return "scrapper", serve.scrap, serve.scrap_panel.panel
    m = next((m for m in serve.SIMPLE if getattr(m, "ELITE", None) == elite), None)
    return (m.ROUTE, m, m.compare) if m else None


def compare_live(body):
    with _lock:
        reload_if_changed()
        j = judge_for(body.get("elite"))
        if j is None:
            return {"kind": None}
        kind, mod, fn = j
        r = live.record(body.get("events", []), body.get("elite"))
        if r is None or not r["casts"]:
            return {"kind": kind, "data": None}
        if hasattr(mod, "live"):
            r = mod.live(r)
        read = evtc_rot.read
        evtc_rot.read = lambda path: r
        try:
            return {"kind": kind, "data": fn(Path("live.zevtc"))}
        finally:
            evtc_rot.read = read


def save_capture(body):
    """Saves a pull's live capture next to its log; {"saved": path}."""
    stem = Path(str(body.get("log", ""))).name
    log = evtc_rot.GOLEM / f"{stem}.zevtc"
    if not stem or not log.is_file():
        raise ValueError(f"no golem log {stem!r}")
    out = live.capture_of(log)
    keep = {k: body.get(k) for k in ("log", "elite", "events", "shown")}
    out.write_text(json.dumps({**keep, "source": "addon", "saved": time.strftime("%Y-%m-%d %H:%M:%S")}),
                   encoding="utf8")
    return {"saved": str(out)}


class H(BaseHTTPRequestHandler):
    def send(self, code, body):
        b = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(b)))
        self.end_headers()
        self.wfile.write(b)

    def do_GET(self):
        _last[0] = time.time()
        self.send(200, json.dumps({"ok": True}))

    def do_POST(self):
        _last[0] = time.time()
        try:
            n = int(self.headers.get("Content-Length", 0))
            live = json.loads(self.rfile.read(n))
            if self.path == "/capture":
                self.send(200, json.dumps(save_capture(live)))
            else:
                self.send(200, json.dumps(compare_live(live)))
        except Exception as e:  # noqa: BLE001
            try:
                CACHE.mkdir(exist_ok=True)
                with open(CACHE / "live_server.log", "a", encoding="utf8") as f:
                    f.write(time.strftime("%Y-%m-%d %H:%M:%S ") + traceback.format_exc() + "\n")
                (CACHE / "live_fail.json").write_text(json.dumps(live) if "live" in locals() else "", encoding="utf8")
            except OSError:
                pass
            self.send(500, json.dumps({"error": f"{type(e).__name__}: {e}"}))

    def log_message(self, *a):
        pass


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8772
    srv = ThreadingHTTPServer(("127.0.0.1", port), H)

    def idle():
        while time.time() - _last[0] < IDLE_EXIT_S:
            time.sleep(10)
        srv.shutdown()
    threading.Thread(target=idle, daemon=True).start()
    srv.serve_forever()


if __name__ == "__main__":
    main()
