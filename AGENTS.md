# Notes for agents

Bench Buddy compares a player's Guild Wars 2 golem rotation with a benchmark log. Read `README.md`
for what it does; this file is how it's built and what to watch out for. Users are often not
programmers: explain what you changed in plain words, and show them the page.

## Architecture

Everything is Rust, in one cargo workspace (`Cargo.toml` at the top); the pages are HTML in
`modules/`. The judging was ported from a Python checker and kept its numbers to the last digit
(Python's rounding, compensated sums and tie orders are in `bb_core::kit`).

- **`crates/core`** (`bb_core`), shared by everything:
  - `evtc.rs` reads a raw arcdps `.zevtc` (a zip holding one binary file): agents, skill names and
    every combat event; `species()` reads only the header's boss id.
  - `rot.rs` turns a log into one player's `Record` (the log's point of view): casts, swaps, buffs,
    hits, damage. Each cast has `id, name, t, exp, base, dur, stop, hits, during, all, missed, dmg`,
    times in ms from the log's first event. `golem_logs(elite, needs)` lists the golem logs of one
    build, oldest first; `logs_dir()` finds the log folder (BENCH_BUDDY_LOGS, else
    `boss_encounter_path` in arcdps.ini, else Windows' Documents folder), and golem logs are found in
    any subfolder by their header.
  - `live.rs` turns the live arcdps feed into the same `Record` (`record()`), with the cast table
    `livecasts.json` (built into the program; `bbtool livecast` measures it).
  - `kit.rs` helpers the builds share, to take or leave: the bench's casts and buffs from its EI JSON,
    `match_casts()` (your casts lined up with the bench's: matched, missed, extra), the cached history
    row, icons, the number formats (`sgn`, `sec`, `clk`, `tone`) and Python's rounding. A helper
    belongs here once two builds need it; a change must keep every build's answers the same.
  - `web.rs` downloads: `bench_log(permalink, dest)` (a dps.report log's Elite Insights JSON, cached)
    and `skills(ids)` (GW2 API). dps.report answers 403 without a User-Agent; always go through it.
  - `build.rs` the `Build` trait: the simple contract below.
- **`crates/berserker`** and **`crates/scrapper`** are the two full builds. They predate the simple
  contract, so their routes are written out in `crates/app` (`/api/compare`, `/api/scrapper/...`).
  Read them for how far a build can go.
- **`crates/app`** (`bb_app::App`) is the judging as one service: `api(path, query)` answers every JSON
  route, `live(body)` judges a running pull from the feed, `save_capture()` keeps a live capture. The
  checker and the addon both use it, so the page and the panel can't disagree.
- **`crates/checker`** is the local server on 127.0.0.1:8771 (`run.bat`, or `cargo run --release -p
  checker -- [port] [--no-browser]`). It reads the pages on every request and the open page reloads
  when one changes; a change to the Rust code ends it with exit code 3, which `run.bat` answers by
  building and starting it again. `checker api <path?query>` prints one route without the server.
- **`crates/tools`** is `bbtool`: `whatlog`, `feedcheck`, `livereplay`, `livecast`, `livetest`.
- **`addon/`** is the Nexus addon. It judges in the game with `bb_app` (no server, no Python) and
  draws; see `addon/README.md`.

## The simple contract (new builds)

A new build is a crate that implements `bb_core::build::Build`, listed in `simple_builds()` in
`crates/app/src/lib.rs` (and in that crate's Cargo.toml). The checker serves:

| Route | Calls |
|---|---|
| `/<route>` | `page()` (default `page.html`) from `modules/<route>/` |
| `/api/<route>/latest` | `logs()`: `{"latest": stem, "count", "name"}` |
| `/api/<route>/history` | `history()` if it gives one, else `[{"log": stem}]` per log |
| `/api/<route>/compare?log=<stem>` | `compare(record, path)` |
| `/api/modules` | every build's name and route |

A build gives `name()`, `route()`, `elite()`, `logs()` and `compare()`; `compare()` returns
`serde_json::Value`. The addon judges it live by its `elite()`, with `live()` to adjust the record
first (the Berserker puts back its weapon swaps). `crates/template` (with `modules/_template/`) is
the starting point; `cargo test --release -p bb_<build> -- --nocapture` prints the newest pull.

Keep a build's code in its own crate. Don't change `crates/core` or another build for one build's
needs: if something belongs in core, say so and keep it general.

## Data facts that cost time to find

- **Casts** are animation start and stop: state changes 67 and 68. This arcdps logs no activation
  events. Logs before October 2026 have none, so no casts (`rot::FIRST_ANIMATED`).
- **Event layout:** 64 bytes; time at offset 0, src 8, dst 16, value 24, buff_dmg 28, skill id 36,
  buff 49, result 50, activation 51, buffremove 52, state change 56. Direct hit: state change 0 with
  buff 0. Weapon swap: state change 11, value the new set. Buff applied to a player: 69; all stacks
  gone: 72. Point of view: 13. Header: boss species at bytes 13-14 (16199 is the golem).
- **A log starts at the first hit**, which itself isn't logged. Casts before it (pre-casts, placed
  bombs) have no start; see `precasts()` in `crates/scrapper/src/lib.rs` for how to put them back.
- **The bench is Elite Insights JSON** (`players[0].rotation`, `skillMap`, `buffUptimes`): every cast
  with its duration, but hits only as totals per skill. A cast cancelled before it did anything has
  `timeGained == -duration`.
- **Kit swaps and many instants have no animation** in your log; drop zero-length casts on both sides
  or they won't match.
- **Elite spec ids** are the GW2 API's specialization ids (18 Berserker, 43 Scrapper, 62 Firebrand).
  `cargo run --release -p bbtool -- whatlog` prints the one in a log.
- **The live feeds** carry no casts or swaps; hits come on the local feed within 20 ms and on the
  area feed 2.65 s late with exact times; buffs only on the area feed (`addon/README.md`).
- **Live vs log:** `bbtool livetest` replays every saved live capture (`<log>.live.json` next to the
  log) and compares it with the log. Run it after changing `crates/core/src/live.rs`,
  `crates/core/livecasts.json` or a build's judging.

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
- **A change that should change nothing:** run `cargo run --release -p checker -- snapshot` before
  and after (`--save` keeps a baseline in `cache/snapshot/`). It judges every pull and live capture
  and shows what changed.
- **Few dependencies.** Each one goes into the addon DLL that players load into the game; prefer the
  standard library, and say why when you add one.
- **Testing without disturbing the user:** they may keep `run.bat` running on 8771, and Windows locks
  the program it runs (`target/release/checker.exe`). Build into your own folder and port:
  `cargo run --release --target-dir target/agent -p checker -- 8791 --no-browser`, compare its JSON
  with theirs, and stop it when done. Saving a `.rs` file restarts their checker with the change.
- **The addon judges with the same code:** after changing a build's judging, build the addon too
  (`cargo build --release -p bench_buddy`) and say that the DLL needs copying into the game.
- **Never commit logs or `cache/` folders:** they hold character names and account data.

## Adding a build to the addon

See `addon/README.md`. In short: a `panel` function in the build's crate reduces a pull to exactly
what is drawn, served at `/api/<route>/panel` by `crates/app`; the addon gets a drawing file like
`addon/src/scrap.rs` and a branch in `lib.rs` that picks it by the newest log. The page's judging and
the panel's must stay the same code path.
