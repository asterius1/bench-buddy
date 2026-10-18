# Bench Buddy

Bench Buddy compares your Guild Wars 2 golem rotation with a benchmark log, cast by cast. It shows
the result in your browser and, through a Nexus addon, in the game.

Do a pull on the Standard Kitty Golem with arcdps logging, and a few seconds after the golem dies the
page shows each of your loops next to the bench's. You see where you lost time, which casts came late
or never landed, which skills sat ready too long, and how far behind the bench you finished.

Nothing is uploaded. Bench Buddy reads the logs arcdps already writes on your PC. It downloads only
the bench log, once, from dps.report.

## Builds

| Build | Page | In game |
|---|---|---|
| Condi Berserker (longbow + sword/torch) | http://127.0.0.1:8771/berserker | Splits panel, fills in live during a pull |
| Power Scrapper (hammer) | http://127.0.0.1:8771/scrapper | Cycle grid and the bench cheat sheet |

Your build isn't here yet? See [Add your own build](#add-your-own-build) below. It's written so you
can do it with an AI coding agent even if you don't program.

## What you need

- Windows, Guild Wars 2 and [arcdps](https://www.deltaconnected.com/arcdps/) with logging on. Logs
  from October 2026 on, since older ones have no cast events. Bench Buddy finds them where arcdps
  writes them: the log folder set in arcdps's options (`arcdps.ini`), else
  `Documents\Guild Wars 2\addons\arcdps\arcdps.cbtlogs`, in any subfolder. To point it somewhere
  else, set the environment variable `BENCH_BUDDY_LOGS` to that folder.
- For the in-game panel: [Nexus](https://raidcore.gg/Nexus) and the addon DLL. The addon judges by
  itself: nothing else has to run while you play (see [addon/README.md](addon/README.md)).
- For the pages in the browser, and to build the addon: [Rust](https://rustup.rs) with its default
  Windows (MSVC) setup.

## Start

1. Download this repository (Code > Download ZIP, or `git clone`).
2. Double-click `run.bat`. The first time, it builds the checker, which takes a minute or two. Then a
   browser tab opens on http://127.0.0.1:8771/, which shows the build of your newest golem log
   (switch builds in the top right corner). Leave the window running.
3. Do a golem pull. When arcdps has written the log, the page shows it by itself.

The first start downloads the bench logs, which takes a few seconds. Click a row in the pull list to
open an earlier pull.

## How it works

```
arcdps log (.zevtc)
   -> crates/core                   your casts, hits, weapon swaps and buffs
   -> crates/<build>                lined up with the bench log and judged
   -> crates/app                    the same judging for both of these:
        -> crates/checker (run.bat)     the pages and their JSON on 127.0.0.1:8771
        -> addon/                       the panel in game, and live judging during a pull
```

Everything is in Rust, in one workspace (`Cargo.toml` at the top). The pages are HTML files.

| Folder | What's in it |
|---|---|
| `crates/core/` | The log reader, the live record from the arcdps feed, the helpers the builds share, downloads. |
| `crates/berserker/`, `crates/scrapper/` | The two builds' judging. |
| `crates/template/` | A small working build to copy for a new one. |
| `crates/app/` | The judging as one service, used by the checker and the addon. |
| `crates/checker/` | The local server behind the pages (`run.bat`). |
| `crates/tools/` | `bbtool`: what your logs hold (`whatlog`), what the live feed carries (`feedcheck`), the live cast table (`livecast`), a recording judged against its logs (`livereplay`), live against the log (`livetest`). |
| `modules/berserker/` | Condi Berserker's pages and views. [README](modules/berserker/README.md) |
| `modules/scrapper/` | Power Scrapper's pages. [README](modules/scrapper/README.md) |
| `modules/_template/` | The template build's page. |
| `addon/` | The Nexus addon. [README](addon/README.md) |
| `AGENTS.md` | Notes for AI coding agents: architecture, the build contract, rules of thumb. |

## Add your own build

A build is a crate under `crates/` that reads your logs of one build and compares them with that
build's benchmark log, plus its page in `modules/`. The template does the basics: it lines up your casts with the bench's in order
and shows how far behind each one started. You then teach it how your rotation works, step by step.

### 1. Get a bench log

You need a golem log of someone playing the build well, on [dps.report](https://dps.report). The
build guides on [Snow Crows](https://snowcrows.com) link a benchmark log for each build. Open it and
copy the end of the address: for `https://dps.report/55xd-20260422-192633_golem` that's
`55xd-20260422-192633_golem`.

Also do a few golem pulls yourself with arcdps on, so there are logs to test with.

### 2. Copy the template

Copy `crates/template` to a new folder, for example `crates/firebrand`, and `modules/_template` to
`modules/firebrand`. In `crates/firebrand/Cargo.toml`, change the name to `bb_firebrand`. Then list
it in `crates/app`: a line `bb_firebrand = { path = "../firebrand" }` under `[dependencies]` in its
`Cargo.toml`, and a line in `simple_builds()` in `crates/app/src/lib.rs` (the comment there shows it).
An agent can do this step for you: ask it to "add a build from the template called firebrand".

### 3. Find your build's numbers

Run this in the repository folder:

```
cargo run --release -p bbtool -- whatlog
```

It prints your newest golem logs with your elite spec id (`ELITE = 62`) and your most cast skills
with their ids. Pick one or two skills that only your build casts. Those go in `SIGNATURE`, and they
are how Bench Buddy tells your Firebrand logs from your other characters' logs.

### 4. Fill in the settings

Open `crates/firebrand/src/lib.rs` and change the lines under "the settings to fill in":

```rust
pub const NAME: &str = "Quickness Firebrand";
pub const ROUTE: &str = "firebrand";       // the page will be http://127.0.0.1:8771/firebrand
pub const ELITE: u32 = 62;
pub const SIGNATURE: [i64; 2] = [41258, 40624];   // skill ids from whatlog (these two are made up)
pub const PERMALINK: &str = "abcd-20260101-120000_golem";   // made up too: use your bench log's
```

Save. The `run.bat` window builds and starts the checker again by itself and prints your build's
address. Open it: you should see your newest pull lined up with the bench.

If it says "waiting for a log of this build", `ELITE` or `SIGNATURE` don't match your logs. Run
`cargo test --release -p bb_firebrand -- --nocapture` to see your newest pull, or the error, in
plain text.

### 5. Teach it your rotation with an agent

This is where an AI coding agent (Claude Code, Cursor, Copilot...) does the work. Open the repository
folder in it and describe your build the way you would explain it to a new player. The agent reads
`AGENTS.md` for how Bench Buddy works, but it doesn't know your build. That part comes from you.

What to tell it:

- **What a loop is.** What repeats, and what marks the start of a new loop: a weapon swap, a kit,
  one skill. Example: "A loop starts at every Electro-whirl. Three loops make a cycle, and the third
  one ends in the bomb burst."
- **The anchors.** Which skills should be cast the moment they're ready, and their cooldowns. Say
  whether a cooldown counts from the start or the end of the cast. Example: "The 20 s skills are the
  anchor and should keep rolling. Recharge counts from the end of the cast."
- **What a mistake is,** with a number. Example: "Flag a cast that starts more than 50 ms later than
  the bench's at the same point. A Mine Field should land 5 hits."
- **What you want to see.** Example: "One row per loop, a split time per loop against the bench,
  and the skills that were late in red."

Good first messages:

> Read AGENTS.md and modules/firebrand. My build is Quickness Firebrand. A loop is one Mantra cycle:
> it starts at the first Mantra of Potence charge... Split my pulls into loops like that and show one
> row per loop with its time against the same loop of the bench.

> In my last pull, check every Tome of Courage: how many hits each cast landed against the bench's.
> Tell me how many of my pulls had a short one, and show me two examples.

> I don't believe that cooldown. Measure it over all my logs and the bench log, and show me the
> shortest recasts, not just the minimum.

Tips that save a lot of back and forth:

- **One change per message.** Look at the page after each one: it reloads by itself a few seconds
  after the code changes (the checker builds again first).
- **Ask for counts and examples,** like "how many of my pulls fit this, and show me one that doesn't."
  Guesses presented as facts are the most common way a module goes wrong.
- **Don't take a cooldown from a single recast.** Ask for the spread over many casts.
- **Say what you see in game** when the log can't show it ("I pre-cast this before the golem
  activates"). The agent should record it as "seen in play".
- **Keep to your folder.** Ask the agent not to change `crates/core` or another build unless it has
  to, and to say so when it does.

### 6. In game (optional)

The addon already judges every listed build, live too. Drawing a new build's own panel in game
needs a few changes to the addon. Ask your agent to follow "Adding a build to the addon" in
[addon/README.md](addon/README.md).

## Contributing

New builds, fixes and better pages are welcome. Open a pull request with your build's crate and page
folder, and a `README.md` in the page folder that says which bench log it uses, how it splits the
rotation, and what it flags.
Please don't commit logs or `cache/` folders: they hold character names.

## Credits

Benchmark logs and rotations from [Snow Crows](https://snowcrows.com), hosted on
[dps.report](https://dps.report) and parsed by
[Elite Insights](https://github.com/baaron4/GW2-Elite-Insights-Parser). Logs by
[arcdps](https://www.deltaconnected.com/arcdps/). The addon runs on [Nexus](https://raidcore.gg/Nexus)
through [nexus-rs](https://github.com/zerthox/nexus-rs). Skill icons and recharges from the
[Guild Wars 2 API](https://wiki.guildwars2.com/wiki/API:Main).

Bench Buddy is a fan project, not affiliated with ArenaNet. Guild Wars 2 is a trademark of ArenaNet,
LLC. MIT licensed, see [LICENSE](LICENSE).
