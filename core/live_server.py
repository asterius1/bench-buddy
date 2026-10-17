"""Judges a pull while it runs, with the checker's own code, for the in-game addon.

    pythonw core/live_server.py [port]      (default 8772; the addon starts it when it is not running)

The addon POSTs to /compare the casts it has put back from your live hits and buffs:
    {"casts": [{"id", "t", "hits"}], "buffs": {buff id: [apply times]}, "start": t, "end": t}
(ms, arcdps clock; start is the pull's first hit, when arcdps starts the log)
and gets back what the checker's /api/compare gives for a log, so the panel draws a running pull
exactly like a finished one. It reads the Berserker module (local, baseline) and evtc_rot, never changes it:
`local.compare()` is called with `evtc_rot.read` answering with the live pull. Live casts have no
lengths, so each gets the bench's median length for its skill; what is judged (start to next cast,
hits, buffs) does not depend on that. Casts started before the first hit are left out, as the log
leaves them out. Weapon swaps are put SWAP_MS before the first cast on the other weapon (the median
over 286 swaps in the golem logs). An auto attack with fewer hits than expected counts as cut, as
the checker counts a cancelled one.
The rotation modules are reloaded when they change, as core/serve.py does. The server exits
after 15 minutes without a request. A request that fails is logged with its traceback to
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
ROOT = HERE.parent
for d in (HERE, ROOT / "modules" / "berserker"):
    sys.path.insert(0, str(d))
import baseline  # noqa: E402
import evtc_rot  # noqa: E402
import kit  # noqa: E402
import local  # noqa: E402

MODULES = [evtc_rot, kit, baseline, local]   # reloaded in this order
IDLE_EXIT_S = 15 * 60
CACHE = HERE / "cache"
SWAP_MS = 116
_lock = threading.Lock()
_last = [time.time()]
_mtimes = {}


def reload_if_changed():
    changed = [m for m in MODULES if Path(m.__file__).stat().st_mtime != _mtimes.get(m.__name__)]
    if not changed:
        return
    if _mtimes:
        for m in MODULES:
            importlib.reload(m)
    for m in MODULES:
        _mtimes[m.__name__] = Path(m.__file__).stat().st_mtime


def pull_record(live):
    """The live casts as `evtc_rot.read()` would give a log."""
    rf, _ = local.ref()
    casts = []
    start = live.get("start", 0)
    for c in sorted(live.get("casts", []), key=lambda c: c["t"]):
        if c["t"] < start:
            continue
        sid, hits = int(c["id"]), c.get("hits", 0)
        dur = round(rf[sid]["med"]) if sid in rf else 300
        cut = sid in baseline.AUTOS and sid in baseline.EXPECT and hits < baseline.EXPECT[sid][0]
        casts.append({"id": sid, "name": baseline.NAMES.get(sid, str(sid)), "t": c["t"], "exp": dur, "base": dur,
                      "dur": dur, "stop": "cancel" if cut else "done", "hits": hits, "during": hits,
                      "all": hits, "missed": 0, "dmg": 0})
    swaps, weapon = [], None
    for a, b in zip(casts, casts[1:]):
        wa = "sword" if a["id"] in local.SWORD else "bow" if a["id"] in local.BOW else weapon
        wb = "sword" if b["id"] in local.SWORD else "bow" if b["id"] in local.BOW else wa
        weapon = wa
        if wa and wb and wa != wb:
            swaps.append({"t": max(a["t"] + 1, b["t"] - SWAP_MS), "set": 0})
    buffs = {int(k): v for k, v in live.get("buffs", {}).items()}
    end = max([live.get("end", 0)] + [c["t"] + c["dur"] for c in casts])
    return {"player": "", "elite": 18, "casts": casts, "swaps": swaps, "buffs": buffs, "buffs_gone": {},
            "loose": {}, "end": end, "file": "live"}


def compare_live(live):
    with _lock:
        reload_if_changed()
        r = pull_record(live)
        if not r["casts"]:
            return None
        read, damage = evtc_rot.read, local.damage
        evtc_rot.read = lambda path: r
        local.damage = lambda path: None
        try:
            return local.compare(Path("live.zevtc"))
        finally:
            evtc_rot.read, local.damage = read, damage


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
