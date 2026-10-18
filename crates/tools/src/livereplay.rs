//! A recorded live feed judged as the addon would, next to the log of the same pull.
//!
//!     bbtool livereplay [SESSION_DIR]
//!
//! SESSION_DIR is a feed recording of the addon (default the newest). For every golem log written
//! while it recorded, the feed's events of that pull are judged live and the log as a log, and the two
//! are printed side by side: per cycle (Scrapper) or row (Berserker) its length and the casts matched,
//! missed and extra. Live can only be as good as the casts it can put back from hits.

use bb_app::App;
use bb_core::omap::OMap;
use bb_core::{evtc, rot};
use serde_json::{json, Value};

use crate::feed;
use crate::py::{of, Py};

/// Chip kinds over the rows' parts, in the order first seen.
pub fn chips(rows: &Value) -> Py {
    let mut c: OMap<String, i64> = OMap::default();
    for r in rows.as_array().into_iter().flatten() {
        for k in ["opener", "sword", "bow"] {
            for s in r.get(k).and_then(|m| m["sections"].as_object()).into_iter().flatten().map(|(_, s)| s) {
                for x in s["chips"].as_array().into_iter().flatten() {
                    *c.entry_or(x["kind"].as_str().unwrap_or("").to_string(), 0) += 1;
                }
            }
        }
    }
    Py::Dict(c.0.into_iter().map(|(k, n)| (Py::Str(k), Py::Int(n))).collect())
}

/// A Scrapper cycle as (number, length s, hits against the bench).
pub fn cycle(g: &Value) -> Py {
    Py::Tuple(vec![of(&g["n"]), g["len"].as_f64().filter(|&l| l != 0.0).map(|l| Py::Float(bb_core::kit::round_n(l / 1000.0, 2))).unwrap_or(Py::None),
                   of(&g["hits"]["net"])])
}

pub fn run(args: &[String], app: &App) -> Result<(), String> {
    let sess = feed::session_arg(args.first())?;
    let evs = feed::feed_events(&sess.join("combat.jsonl"));
    if evs.is_empty() {
        println!("no feed events in {}", sess.display());
        return Ok(());
    }
    let (lo, hi) = (evs[0]["time"].as_i64().unwrap_or(0), evs[evs.len() - 1]["time"].as_i64().unwrap_or(0));
    for p in rot::all_golem_logs() {
        let Ok(log) = evtc::load(&p) else { continue };
        let (Some(a), Some(b)) = (log.events.first(), log.events.last()) else { continue };
        let (a, b) = (a.time as i64, b.time as i64);
        if !(lo <= a && a <= hi) {
            continue;
        }
        let elite = rot::read(&p)?.elite;
        let kind = match elite { 18 => "berserker", 43 => "scrapper", _ => continue };
        let got = app.live(&json!({"elite": elite, "events": feed::pull_events(&evs, a, b)}))?["data"].clone();
        let stem = rot::stem(&p);
        let rows = |v: &Value| v["rows"].as_array().map_or(0, |r| r.len());
        if kind == "scrapper" {
            let log = app.scrap_panel(&stem)?;
            let list = |v: &Value| Py::List(v["rows"].as_array().into_iter().flatten().map(cycle).collect());
            println!("\n{stem} Scrapper: log {} cycles, live {}   (cycle, length s, hits against the bench)", rows(&log), rows(&got));
            println!("  log  {}", list(&log));
            println!("  live {}", list(&got));
            println!("  behind log {}  live {}", log["head"][0][1].as_str().unwrap_or(""),
                     got["head"][0][1].as_str().map(String::from).unwrap_or_else(|| "None".into()));
        } else {
            let log = app.bers_compare(&stem)?;
            let lag = |v: &Value| Py::List(v["lag"].as_array().into_iter().flatten().take(8).map(of).collect());
            println!("\n{stem} Berserker: log {} rows, live {}", rows(&log), rows(&got));
            println!("  log chips:  {}", chips(&log["rows"]));
            println!("  live chips: {}", chips(&got["rows"]));
            println!("  lag log  {}", lag(&log));
            println!("  lag live {}", lag(&got));
        }
    }
    Ok(())
}
