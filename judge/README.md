# Bench Buddy judging in Rust

The checker's judging ported to Rust, one build at a time, so the addon can judge without Python and
the pages and the addon still run the same code. So far: the Power Scrapper (`modules/scrapper/`:
`compare()`, the addon panel and the bench cheat sheet), with the log reading it needs
(`core/evtc.py`, `core/evtc_rot.py`, `core/kit.py`).

| File | Ported from |
|---|---|
| `src/evtc.rs` | `core/evtc.py` |
| `src/rot.rs` | `core/evtc_rot.py` |
| `src/kit.rs`, `src/difflib.rs` | `core/kit.py` (and Python's `difflib`, for the same cast matching) |
| `src/scrapper/mod.rs` | `modules/scrapper/scrap.py` |
| `src/scrapper/panel.rs`, `src/scrapper/sheet.rs` | `modules/scrapper/panel.py` and `bench_sheet()` |

It gives the same JSON as the Python checker. Python's rounding is kept (half to even), and so are
its tie orders.

## Check it

    python tools/judgeparity.py

builds it and compares every Scrapper pull's `compare()` and panel, and the cheat sheet, with the
Python checker. Run it after changing either side; while both exist, a rule changes in both.

    cargo run --release --bin bbjudge -- compare <log.zevtc>

(from the repository root) prints one pull. `bbjudge panel`, `sheet`, `logs` and `dump <dir>` too.
The bench is read from `modules/scrapper/cache/bench.json`, which the Python checker downloads on
its first run.

Not ported yet: the history page's rows, the bench download and icons, the live judge, and the
Berserker.
