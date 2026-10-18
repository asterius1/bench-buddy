# Bench Buddy addon (Nexus)

The checker's report inside the game: a [Nexus](https://raidcore.gg/Nexus) addon in Rust on
[nexus-rs](https://github.com/zerthox/nexus-rs). It judges with the checker's own code
(`crates/app`), built into the DLL, so nothing else has to run. Every 2 s it looks for the newest
golem log of either build and shows the newer one: the Splits panel for a Condi Berserker pull, the
cycle grid for a Power Scrapper pull (with a cheat sheet button for the bench rotation).
CTRL+SHIFT+F8 shows or hides it; arrows at the top go to earlier pulls.

It reads nothing from game memory and sends no input: only the arcdps combat events Nexus passes to
every addon, and the arcdps logs on your PC. It downloads each build's bench log once from
dps.report, and skill icons from the GW2 API.

## Build and install

1. Install Rust with the MSVC toolchain (https://rustup.rs).
2. `cargo build --release -p bench_buddy` in the repository.
3. Copy `target/release/bench_buddy.dll` into the game's `addons` folder
   (`C:\Program Files\Guild Wars 2\addons\` by default) and load "Bench Buddy" once in Nexus's addon
   list. Later copies are reloaded while the game runs.

`<GW2>/addons/bench_buddy/settings.json` holds the filler mode and the capture setting;
`<GW2>/addons/bench_buddy/cache/` the bench logs, icons and one summary per pull. The logs are found
through arcdps's own settings (`boss_encounter_path` in `addons/arcdps/arcdps.ini`), else in Windows'
Documents folder; the options page shows the folder used.

## Live (every build)

While a pull runs the panel fills in. The arcdps feeds carry your hits but no casts, cancels or
weapon swaps (no cast start or end at all). Every direct hit of yours comes on both feeds: on the
local feed about 20 ms after it happened, on the area feed 2.65 s after, with the exact time. The
addon takes the local hit at once and lets the area copy correct its time; buffs and condition
ticks come from the area feed only. Once a second it judges the events so far.

The casts are put back from the hits (`crates/core/src/live.rs`): a damaging skill's first hit
comes a fixed time after its cast starts (Scorched Earth 318 ms, Electro-whirl, Blaze Breaker 400,
spread about 20 ms), and a skill that does no damage by a buff it puts on you at a fixed time (Flames
of War, a Scrapper's dodge, Reconstruction Field by Protection with Superspeed). `bbtool livecast`
measures both from all your golem logs into `crates/core/livecasts.json` and checks them on a
recording: on a Scrapper session 185 of 210 casts came back, none wrong, starts within 2 ms
(median); the rest were cancelled autos that never hit and Medic Gyro (no hit, no steady buff).
Then the build's own crate judges the pull as it judges a log, so a build gets live with no code of
its own; `bbtool livereplay` holds a recording against the logs (a Scrapper's cycle lengths came out
within 10 ms).

**Live captures** (off by default, in the addon's options in Nexus): when a pull's log comes in,
the events the addon last judged for it, and what it drew, are saved next to the log as
`<log>.live.json`. Nothing leaves the PC. `bbtool livetest` then holds the live path against the log
for every pull that has both: casts found, missed and extra per skill, and whether the build judged
the pull the same. `--import` cuts captures out of the older feed recordings in `sessions/`;
`--save` keeps the run as a baseline, and later runs exit with 1 when a pull got worse.

When the log arrives, the real report replaces the live one. The title row shows the live state
(grey ready, green judging, amber waiting, red with the error).

## Files

| File | What it does |
|---|---|
| `src/lib.rs` | Load and unload, the poller for the newest log, the live judging thread, the arcdps feed. |
| `src/report.rs` | Reduces the Berserker `/api/compare` report to what is drawn, following `modules/berserker/views/splits.html`. |
| `src/panel.rs` | Draws the Berserker Splits panel and its hover cards. |
| `src/scrap.rs` | Draws the Scrapper grid from `/api/scrapper/panel`, already reduced by `crates/scrapper/src/panel.rs`. |
| `src/live.rs` | Keeps the running pull's feed events for the live judging, and records your feed events to `addons/bench_buddy/sessions/` for `bbtool feedcheck`. |

## Tests

`cargo test --release -p bench_buddy` runs the offline tests. Some only run when asked:
`BB_SCRAP=1` (judges the newest scrapper pull and walks the fields drawn), `BB_REPORT=<saved
/api/compare json>` and `BB_REPLAY=<combat.jsonl>` with `BB_ELITE=<spec id>` (judges a recorded
session as the addon does). Add `-- --nocapture` to see their output.

## Adding a build to the addon

A new build is judged live in the game as soon as it is listed in `crates/app`. To draw its own
panel, the pattern is the Scrapper's: the build's crate gets a `panel` function that reduces a pull
to exactly what is drawn (texts final, colours decided) behind an `/api/<route>/panel` route in
`crates/app`, and the addon gets a drawing file like `src/scrap.rs` plus a branch in `lib.rs` that
picks it by the newest log. Keeping the decisions in the build's crate means the page and the panel
can't disagree.
