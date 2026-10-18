# Notes for agents

Bench Buddy compares a player's Guild Wars 2 golem rotation with a benchmark log. Read `README.md`
for what it does; this file is how it's built and what to watch out for. Users are often not
programmers: explain what you changed in plain words, and show them the page.

## Architecture

- **`core/evtc.py`** reads a raw arcdps `.zevtc` (a zip holding one binary file): agents, skill
  names and every combat event. Standard library only.
- **`core/evtc_rot.py`** turns a log into one player's record (the log's point of view):
  `read(path)` returns `{"player", "elite", "casts", "swaps", "buffs", "buffs_gone", "loose", "end", "file"}`.
  Each cast is `{"id", "name", "t", "exp", "base", "dur", "stop", "hits", "during", "all", "missed", "dmg"}`,
  times in ms from the log's first event. `golem_logs(elite, needs)` lists the Standard Kitty Golem
  logs of one build, oldest first. `GOLEM` is the folder.
- **`core/kit.py`** helpers the modules share, to take or leave: the bench's casts and buffs from
  its EI JSON, `match()` (your casts lined up with the bench's: matched, missed, extra), the cached
  history row, icons, and the number formats (`sgn`, `sec`, `clk`, `tone`). A helper belongs here
  once two builds need it; a change must keep every module's output the same on every log.
- **`core/web.py`** downloads: `bench_log(permalink, dest)` (a dps.report log's Elite Insights JSON,
  cached) and `skills(ids)` (GW2 API). dps.report answers 403 without a User-Agent; always go through it.
- **`core/serve.py`** is the local server on 127.0.0.1:8771 (`run.bat`, or `python core/serve.py
  [port] [--no-browser]`). It reloads changed modules by itself and the open page reloads with them;
  a change to `serve.py` itself exits with code 3, which `run.bat` answers by starting it again.
- **`core/live_server.py`** is the live judge on 8772 for the addon: `core/live.py` turns the live
  arcdps feed into the record `evtc_rot.read()` gives for a log, and the build's module (by elite
  spec) judges it with its own `compare()`. A module gets live for free if it reads pulls only
  through `evtc_rot.read()`; an optional `live(record)` adjusts the record (the Berserker's swaps).
- **`modules/berserker/`** and **`modules/scrapper/`** are the two full modules. They predate the
  simple contract below, so their routes are written out in `serve.py` (`/`, `/api/compare`,
  `/scrapper`, `/api/scrapper/...`). Read them for how far a module can go.
- **`judge/`** is the judging ported to Rust, so far the Scrapper (see `judge/README.md`). It must give
  the same JSON as the Python module: `python tools/judgeparity.py` checks every pull.
- **`addon/`** is the Nexus addon in Rust. It only draws; every decision is made in Python. See
  `addon/README.md`.

## The simple contract (new modules)

`serve.py` loads every `modules/<folder>/module.py` whose folder doesn't start with `_`, and serves:

| Route | Calls |
|---|---|
| `/<ROUTE>` | `PAGE` (default `page.html`) from the module's folder |
| `/api/<ROUTE>/latest` | `logs()`: `{"latest": stem, "count", "name"}` |
| `/api/<ROUTE>/history` | `history()` if defined, else `[{"log": stem}]` per log |
| `/api/<ROUTE>/compare?log=<stem>` | `compare(path)`, cached per log until the file or the code changes |
| `/api/modules` | every module's name and route |

A module must define `NAME`, `ROUTE`, `logs()` and `compare(path)`. `compare()` returns plain JSON
(dicts, lists, numbers, strings). A module that fails to import is reported in the `run.bat` window
and skipped. `modules/_template/` is the starting point; `python modules/<folder>/module.py` prints
the newest pull without the server.

Keep a module's helpers in its own folder, and give them names no other module uses (the folders
share one Python path). Don't change `core/` or another module for one build's needs: if something
belongs in `core/`, say so and keep it general.

## Data facts that cost time to find

- **Casts** are animation start and stop: state changes 67 and 68. This arcdps logs no activation
  events. Logs before October 2026 have none, so no casts (`evtc_rot.FIRST_ANIMATED`).
- **Event layout:** 64 bytes; time at offset 0, src 8, dst 16, value 24, buff_dmg 28, skill id 36,
  result 50, state change 56. Direct hit: state change 0 with buff 0. Weapon swap: state change 11,
  value the new set. Buff applied to a player: 69; all stacks gone: 72. Point of view: 13.
- **A log starts at the first hit**, which itself isn't logged. Casts before it (pre-casts, placed
  bombs) have no start; see `_precasts()` in `modules/scrapper/scrap.py` for how to put them back.
- **The bench is Elite Insights JSON** (`players[0].rotation`, `skillMap`, `buffUptimes`): every cast
  with its duration, but hits only as totals per skill. A cast cancelled before it did anything has
  `timeGained == -duration`.
- **Kit swaps and many instants have no animation** in your log; drop zero-length casts on both sides
  or they won't match.
- **Elite spec ids** are the GW2 API's specialization ids (18 Berserker, 43 Scrapper, 62 Firebrand).
  `tools/whatlog.py` prints the one in a log.
- **The live feeds** carry no casts or swaps; hits come on the local feed within 20 ms and on the
  area feed 2.65 s late with exact times; buffs only on the area feed (`addon/README.md`).
- **Live vs log:** `python tools/livetest.py` replays every saved live capture (`<log>.live.json`
  next to the log) and compares it with the log. Run it after changing `core/live.py`,
  `core/livecasts.json` or a module's judging.

## How to work here

- **Measure, then claim.** A rule about the rotation needs a test over all the user's logs: how many
  casts fit, how many don't, and an example of each. Say which logs and how many. Mark what is
  measured, what is inferred, and what the user saw in play.
- **Cooldowns from a distribution,** not from the single shortest recast: one odd recast (a reset,
  a log hiccup) gives a wrong number. Show the spread. Recharge often counts from the end of the cast;
  check which.
- **When the user doubts a number, recheck the measurement** before defending it.
- **The user's knowledge of the build outranks a thin log sample.** Record it, and say plainly when
  the logs disagree.
- **Pages:** dark colours, `system-ui` font, one short value per cell, details on hover. Look at
  `modules/scrapper/grid.html` for the house style.
- **Python standard library only.** No packages, so a player only needs Python.
- **Testing without disturbing the user:** they may keep `run.bat` running on 8771. Start your own
  copy on another port (`python core/serve.py 8791 --no-browser`), compare its JSON with theirs, and
  stop it when done.
- **Never commit logs or `cache/` folders:** they hold character names and account data.

## Adding a build to the addon

See `addon/README.md`. In short: a `panel.py` in the module reduces a pull to exactly what is drawn,
served at `/api/<route>/panel`; the addon gets a drawing file like `addon/src/scrap.rs` and a branch
in `lib.rs` that picks it by the newest log. The page's judging and the panel's must stay the same
code path.
