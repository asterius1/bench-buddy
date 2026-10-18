# Bench Buddy's crates

| Crate | Folder | What it is |
|---|---|---|
| `bb_core` | `core/` | The log reader, the record of a pull (from a log or the live feed), the helpers the builds share, downloads, the `Build` trait. |
| `bb_berserker` | `berserker/` | Condi Berserker. |
| `bb_scrapper` | `scrapper/` | Power Scrapper, with the addon panel and the bench cheat sheet. |
| `bb_template` | `template/` | The starting point for a new build (not listed in the app). |
| `bb_app` | `app/` | The judging as one service: every JSON route, live judging, captures. |
| `checker` | `checker/` | The pages' server on 127.0.0.1:8771, and `checker api`, `live`, `snapshot`. |
| `bbtool` | `tools/` | The measuring tools: `whatlog`, `feedcheck`, `livereplay`, `livecast`, `livetest`. |

The addon (`addon/`, `bench_buddy`) is in the same workspace and uses `bb_app`.

## Ported from Python

The judging was a Python checker until October 2026. The port kept its answers exactly: on the
last commit with both (`tools/parity.py` there), the Rust and Python checkers gave the same JSON on
174 Berserker and 133 Scrapper pulls, every route, the pages and 35 live captures, and the tools the
same output. What that took, for anyone changing the shared helpers:

- Python's `round()` (half to even) and `round(x, n)`: `kit::round0`, `kit::round_n`.
- Python's `sum()` of floats is compensated since 3.12: `kit::psum`.
- `statistics.median`, `difflib.SequenceMatcher` (`difflib.rs`), and dict order wherever a map is
  walked (`omap.rs`).

`checker snapshot` holds the judging against a saved run of itself, now that the Python is gone.
