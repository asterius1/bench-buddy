"""Downloads for the modules: bench logs from dps.report and skill details from the GW2 API.

dps.report refuses requests without a User-Agent (403), so every download goes through get().
"""
import json
import urllib.request

AGENT = "bench-buddy (Guild Wars 2 golem rotation checker)"


def get(url, timeout=60):
    req = urllib.request.Request(url, headers={"User-Agent": AGENT})
    with urllib.request.urlopen(req, timeout=timeout) as u:
        return u.read()


def bench_log(permalink, dest):
    """A dps.report log's Elite Insights JSON (permalink: the part after dps.report/), downloaded
    to `dest` the first time and read from there afterwards."""
    if not dest.exists():
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(get(f"https://dps.report/getJson?permalink={permalink}"))
    return json.loads(dest.read_text(encoding="utf-8"))


def skills(ids):
    """Skill details (name, icon, recharge ...) from the GW2 API, for a list of skill ids."""
    return json.loads(get("https://api.guildwars2.com/v2/skills?ids=" + ",".join(map(str, ids)), timeout=20))
