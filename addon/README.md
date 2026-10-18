# Bench Buddy addon (Nexus)

The checker's report inside the game: a [Nexus](https://raidcore.gg/Nexus) addon in Rust on
[nexus-rs](https://github.com/zerthox/nexus-rs). It draws; the checker (`core/serve.py`, started by
`run.bat`) decides. Every 2 s it asks the checker on 127.0.0.1:8771 for the newest golem log of
either build and shows the newer one: the Splits panel for a Condi Berserker pull, the cycle grid
for a Power Scrapper pull (with a cheat sheet button for the bench rotation). CTRL+SHIFT+F8 shows or
hides it; arrows at the top go to earlier pulls.

It reads nothing from game memory and sends no input: only the arcdps combat events Nexus passes to
every addon, and the checker's answers.

## Build and install

1. Install Rust with the MSVC toolchain (https://rustup.rs).
2. `cargo build --release` in this folder.
3. Copy `target/release/bench_buddy.dll` into the game's `addons` folder
   (`C:\Program Files\Guild Wars 2\addons\` by default) and load "Bench Buddy" once in Nexus's addon
   list. Later copies are reloaded while the game runs.
4. Start `run.bat` in the repository and leave it running.

`<GW2>/addons/bench_buddy/settings.json` holds the filler mode, and may hold `"repo"`: the checkout
the live judge is started from. Without it the addon asks the running checker where it lives.

## Live (every build)

While a pull runs the panel fills in. The arcdps feeds carry your hits but no casts, cancels or
weapon swaps (no cast start or end at all). Every direct hit of yours comes on both feeds: on the
local feed about 20 ms after it happened, on the area feed 2.65 s after, with the exact time. The
addon takes the local hit at once and lets the area copy correct its time; buffs and condition
ticks come from the area feed only. It sends your events once a second, as they are, to the live
judge, `core/live_server.py` on 127.0.0.1:8772.

The judge puts the casts back (`core/live.py`): a damaging skill's first hit comes a fixed time
after its cast starts (Scorched Earth 318 ms, Electro-whirl, Blaze Breaker 400, spread about 20 ms),
and a skill that does no damage by a buff it puts on you at a fixed time (Flames of War, a
Scrapper's dodge, Reconstruction Field by Protection with Superspeed). `tools/livecast.py` measures
both from all your golem logs into `core/livecasts.json` and checks them on a recording: on a
Scrapper session 185 of 210 casts came back, none wrong, starts within 2 ms (median); the rest
were cancelled autos that never hit and Medic Gyro (no hit, no steady buff). Then the build's own
module judges the pull as it judges a log, so a module that reads its pulls through
`core/evtc_rot.py` gets live with no code of its own; `tools/livereplay.py` holds a recording
against the logs (a Scrapper's cycle lengths came out within 10 ms).

**Live captures** (off by default, in the addon's options in Nexus): when a pull's log comes in,
the events the addon last sent the judge for it, and what it drew, are saved next to the log as
`<log>.live.json`. Nothing leaves the PC. `python tools/livetest.py` then holds the live path against
the log for every pull that has both: casts found, missed and extra per skill, and whether the
build's module judged the pull the same. `--import` cuts captures out of the older feed recordings
in `sessions/`; `--save` keeps the run as a baseline, and later runs exit with 1 when a pull got
worse.

The addon starts the judge with `pythonw` when nothing answers; it exits after 15 minutes idle.
When the log arrives, the real report replaces the live one. The title row shows the live state
(grey ready, green judging, amber waiting, red with the error); errors go to
`core/cache/live_server.log`.

## Files

| File | What it does |
|---|---|
| `src/lib.rs` | Load and unload, the poller for the checker, the live judge thread, the arcdps feed. |
| `src/report.rs` | Reduces the Berserker `/api/compare` report to what is drawn, following `modules/berserker/views/splits.html`. |
| `src/panel.rs` | Draws the Berserker Splits panel and its hover cards. |
| `src/scrap.rs` | Draws the Scrapper grid from `/api/scrapper/panel`, already reduced by `modules/scrapper/panel.py`. |
| `src/live.rs` | Keeps the running pull's feed events for the live judge, and records your feed events to `addons/bench_buddy/sessions/` for `tools/feedcheck.py`. |

## Tests

`cargo test --release` runs the offline tests. Some tests talk to running servers and only run when
asked: `BB_CHECKER=1` (the checker on 8771), `BB_SCRAP=1` (the newest scrapper pull's fields),
`BB_SPAWN=1` (starts the live judge as the addon does), `BB_REPORT=<saved /api/compare json>` and
`BB_REPLAY=<combat.jsonl>` with `BB_ELITE=<spec id>` (feeds a recorded session through the live judge). Add
`-- --nocapture` to see their output.

## Adding a build to the addon

A new module first works in the browser only (see the main README). To draw it in game, the
pattern is the Scrapper's: the module gets a `panel.py` that reduces a pull to exactly what is drawn
(texts final, colours decided) behind an `/api/<route>/panel` route, and the addon gets a drawing file
like `src/scrap.rs` plus a branch in `lib.rs` that picks it by the newest log. Keeping the decisions in
Python means the page and the panel can't disagree.
