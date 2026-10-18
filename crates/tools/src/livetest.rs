//! The live path against the log, for every pull that has both.
//!
//! ```text
//! bbtool livetest                  every pull with a capture: how live and the log differ
//! bbtool livetest STEM...          only these pulls (log stems, e.g. 20261018-022308)
//! bbtool livetest --import [DIR]   cut captures out of the addon's feed recordings first
//! bbtool livetest --save           take this run as the baseline the next runs are held to
//! ```
//!
//! A capture is `<log>.live.json` next to the arcdps log: the events the addon last judged live for
//! that pull, saved when the log came in with "Save live captures" on in the addon's options.
//! `--import` makes them for older pulls from the recordings in `<GW2>/addons/bench_buddy/sessions/`
//! (area feed only, so hit times are exact but nothing is local).
//!
//! For each pull, two things are held side by side:
//!   casts   the record the live path puts back from the feed against the log's. A log cast is
//! ```text
//!       "found" when live has a cast of the same skill within 300 ms, "missed" when not,
//!       "unseen" when live can't see that skill at all (not in the live cast table: heals, kit
//!       swaps). Live casts with no log cast are "extra". Zero-length log casts are left out.
//! ```
//!   judged  the build on the live record (as the addon runs it) against the same build on the log:
//! ```text
//!       Scrapper cycles (length, hits against the bench), Berserker chips.
//! ```
//!
//! With a baseline (`--save`, kept next to the logs as livetest_baseline.json) a pull that finds
//! fewer casts or more extra ones than before is marked WORSE and the run exits with 1.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bb_app::App;
use bb_core::omap::OMap;
use bb_core::rot::{self, Cast};
use bb_core::{evtc, live};
use serde_json::{json, Value};

use crate::feed;
use crate::livereplay::{chips, cycle};
use crate::py::{dumps, median, most_common, Py};

const MATCH_MS: i64 = 300;
const LEN_TOL_S: f64 = 0.05; // cycle lengths this close count as the same (live times are exact to ~2 ms)

fn golem_dir() -> PathBuf {
    let d = rot::logs_dir().join("Standard Kitty Golem (16199)");
    if d.is_dir() { d } else { rot::logs_dir() }
}

fn baseline() -> PathBuf {
    golem_dir().join("livetest_baseline.json")
}

/// Writes a capture for every golem log a feed recording covers and that has none yet.
fn import_sessions(dirs: &[PathBuf]) {
    let dirs: Vec<PathBuf> = if dirs.is_empty() { feed::sessions() } else { dirs.to_vec() };
    let logs = rot::all_golem_logs();
    let mut made = 0;
    for d in &dirs {
        let f = d.join("combat.jsonl");
        if std::fs::metadata(&f).map_or(true, |m| m.len() == 0) {
            continue;
        }
        let evs = feed::feed_events(&f);
        if evs.is_empty() {
            continue;
        }
        let (lo, hi) = (evs[0]["time"].as_i64().unwrap_or(0), evs[evs.len() - 1]["time"].as_i64().unwrap_or(0));
        for p in &logs {
            let out = live::capture_of(p);
            if out.exists() {
                continue;
            }
            let Ok(log) = evtc::load(p) else { continue };
            let (Some(a), Some(b)) = (log.events.first(), log.events.last()) else { continue };
            let (a, b) = (a.time as i64, b.time as i64);
            if !(lo <= a && a <= hi) {
                continue;
            }
            let Ok(r) = rot::read(p) else { continue };
            let body = json!({"log": rot::stem(p), "elite": r.elite, "events": feed::pull_events(&evs, a, b),
                              "source": format!("session {}", d.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())});
            if std::fs::write(&out, body.to_string()).is_ok() {
                made += 1;
            }
        }
    }
    println!("imported {made} captures from {} recordings", dirs.len());
}

/// found [(log cast, live cast)], missed, unseen, extra.
fn match_casts<'a>(log: &'a [Cast], got: &'a [Cast], seen: &HashSet<i64>) -> (Vec<(&'a Cast, &'a Cast)>, Vec<&'a Cast>, Vec<&'a Cast>, Vec<&'a Cast>) {
    let log: Vec<&Cast> = log.iter().filter(|c| c.dur.unwrap_or(0) > 0).collect();
    let mut free: Vec<&Cast> = got.iter().collect();
    free.sort_by_key(|c| c.t);
    let mut pairs = vec![];
    for (i, a) in log.iter().enumerate() {
        for (j, b) in free.iter().enumerate() {
            if a.id == b.id && (a.t - b.t).abs() <= MATCH_MS {
                pairs.push(((a.t - b.t).abs(), i, j));
            }
        }
    }
    pairs.sort();
    let (mut li, mut lj, mut found) = (HashSet::new(), HashSet::new(), vec![]);
    for (_, i, j) in pairs {
        if !li.contains(&i) && !lj.contains(&j) {
            li.insert(i);
            lj.insert(j);
            found.push((log[i], free[j]));
        }
    }
    let rest: Vec<&Cast> = log.iter().enumerate().filter(|(i, _)| !li.contains(i)).map(|(_, c)| *c).collect();
    let missed = rest.iter().copied().filter(|c| seen.contains(&c.id)).collect();
    let unseen = rest.iter().copied().filter(|c| !seen.contains(&c.id)).collect();
    let extra = free.iter().enumerate().filter(|(j, _)| !lj.contains(j)).map(|(_, c)| *c).collect();
    (found, missed, unseen, extra)
}

/// A short, comparable summary of what the build made of a pull.
fn judged(kind: &str, data: &Value) -> Py {
    if data.is_null() {
        return Py::None;
    }
    match kind {
        "scrapper" => Py::List(data["rows"].as_array().into_iter().flatten().map(cycle).collect()),
        _ => match chips(&data["rows"]) {
            Py::Dict(mut v) => {
                v.sort_by(|a, b| a.0.to_string().cmp(&b.0.to_string()));
                Py::Dict(v)
            }
            x => x,
        },
    }
}

fn same(kind: &str, a: &Py, b: &Py) -> bool {
    if let (true, Py::List(x), Py::List(y)) = (kind == "scrapper", a, b) {
        if !x.is_empty() && !y.is_empty() && x.len() == y.len() {
            return x.iter().zip(y).all(|(p, q)| match (p, q) {
                (Py::Tuple(p), Py::Tuple(q)) => p[0] == q[0] && p[2] == q[2] && (p[1] == q[1] || match (&p[1], &q[1]) {
                    (Py::Float(l), Py::Float(m)) => (l - m).abs() <= LEN_TOL_S,
                    _ => false,
                }),
                _ => false,
            });
        }
    }
    a == b
}

fn names(cs: &[&Cast]) -> String {
    let mut c: OMap<String, i64> = OMap::default();
    for x in cs {
        *c.entry_or(x.name.clone(), 0) += 1;
    }
    most_common(&c).into_iter().take(4).map(|(n, k)| format!("{n} x{k}")).collect::<Vec<_>>().join(", ")
}

struct One {
    log: String,
    kind: &'static str,
    source: String,
    found: usize,
    missed: usize,
    unseen: usize,
    extra: usize,
    dt: Option<i64>,
    dt_spread: Option<(i64, i64)>,
    swaps: Option<(usize, usize)>,
    names: [(&'static str, String); 3],
    by_skill: Vec<(&'static str, Vec<String>)>,
    judged_log: Py,
    judged_live: Py,
    same: bool,
}

fn one(path: &Path, app: &App) -> Result<Result<One, String>, String> {
    let cap: Value = serde_json::from_str(&std::fs::read_to_string(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let stem = path.file_name().unwrap().to_string_lossy().trim_end_matches(".live.json").to_string();
    let log_path = path.with_file_name(format!("{stem}.zevtc"));
    let elite = cap["elite"].as_u64().map(|e| e as u32);
    let kind = match elite {
        Some(18) => "berserker",
        Some(43) => "scrapper",
        e => return Ok(Err(format!("no module for elite {}", e.map(|x| x.to_string()).unwrap_or_else(|| "None".into())))),
    };
    let want = rot::read(&log_path)?;
    let tab = live::table(elite, None);
    let events = live::events_of(&cap["events"]);
    let mut rec = live::record(&events, elite, &tab);
    if kind == "berserker" {
        let b = app.bers()?;
        rec = rec.map(|r| bb_berserker::live(r, &b));
    }
    let empty = vec![];
    let got_casts = rec.as_ref().map(|r| &r.casts).unwrap_or(&empty);
    let seen: HashSet<i64> = tab.keys().copied().collect();
    let (found, missed, unseen, extra) = match_casts(&want.casts, got_casts, &seen);
    let dt: Vec<i64> = found.iter().map(|(a, b)| b.t - a.t).collect();
    let got = app.live(&json!({"elite": elite, "events": cap["events"]}))?["data"].clone();
    let log_data = if kind == "scrapper" { app.scrap_panel(&stem)? } else { app.bers_compare(&stem)? };
    let (a, b) = (judged(kind, &log_data), judged(kind, &got));
    let by = |v: &[&Cast]| v.iter().map(|c| c.name.clone()).collect::<Vec<_>>();
    Ok(Ok(One {
        log: stem, kind, source: cap["source"].as_str().unwrap_or("").into(),
        found: found.len(), missed: missed.len(), unseen: unseen.len(), extra: extra.len(),
        dt: (!dt.is_empty()).then(|| bb_core::kit::round0(median(&dt.iter().map(|&x| x as f64).collect::<Vec<_>>())) as i64),
        dt_spread: (!dt.is_empty()).then(|| (*dt.iter().min().unwrap(), *dt.iter().max().unwrap())),
        swaps: (kind == "berserker").then(|| (want.swaps.len(), rec.as_ref().map_or(0, |r| r.swaps.len()))),
        names: [("missed", names(&missed)), ("extra", names(&extra)), ("unseen", names(&unseen))],
        by_skill: vec![("missed", by(&missed)), ("extra", by(&extra)), ("unseen", by(&unseen))],
        same: same(kind, &a, &b), judged_log: a, judged_live: b,
    }))
}

pub fn run(args: &[String], app: &App) -> Result<i32, String> {
    let mut args = args.to_vec();
    if let Some(i) = args.iter().position(|a| a == "--import") {
        let dirs: Vec<PathBuf> = args[i + 1..].iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).collect();
        import_sessions(&dirs);
        args.truncate(i);
    }
    let save = args.iter().any(|a| a == "--save");
    let stems: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let mut caps: Vec<PathBuf> = rot::all_golem_logs().iter().map(|p| live::capture_of(p)).filter(|c| c.is_file()).collect();
    caps.sort_by_key(|c| c.file_name().map(|f| f.to_os_string()));
    if !stems.is_empty() {
        caps.retain(|c| stems.iter().any(|s| c.file_name().unwrap().to_string_lossy().starts_with(s.as_str())));
    }
    if caps.is_empty() {
        println!("no captures next to the logs in {}", golem_dir().display());
        println!("turn on \"Save live captures\" in the addon options, or run with --import");
        return Ok(0);
    }
    let base: Value = if save { Value::Null } else {
        std::fs::read_to_string(baseline()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null)
    };
    let (mut tot, mut worse, mut out) = (OMap::<&str, i64>::default(), vec![], serde_json::Map::new());
    let mut skills: Vec<(&str, OMap<String, i64>)> = vec![("missed", OMap::default()), ("extra", OMap::default()), ("unseen", OMap::default())];
    for c in &caps {
        let r = match one(c, app) {
            Ok(Ok(r)) => r,
            Ok(Err(skip)) => {
                println!("\n{}: skipped, {skip}", c.file_name().unwrap().to_string_lossy().trim_end_matches(".live.json"));
                continue;
            }
            Err(e) => {
                let n = c.file_name().unwrap().to_string_lossy().into_owned();
                println!("\n{n}: FAILED {e}");
                worse.push(n);
                continue;
            }
        };
        out.insert(r.log.clone(), json!({"found": r.found, "missed": r.missed, "extra": r.extra}));
        for (k, v) in [("found", r.found), ("missed", r.missed), ("unseen", r.unseen), ("extra", r.extra)] {
            *tot.entry_or(k, 0) += v as i64;
        }
        *tot.entry_or("pulls", 0) += 1;
        *tot.entry_or("same", 0) += r.same as i64;
        for (k, v) in &r.by_skill {
            let m = &mut skills.iter_mut().find(|s| s.0 == *k).unwrap().1;
            for n in v {
                *m.entry_or(n.clone(), 0) += 1;
            }
        }
        let b = &base[&r.log];
        let mut flag = String::new();
        if let (Some(bf), Some(be)) = (b["found"].as_i64(), b["extra"].as_i64()) {
            if (r.found as i64) < bf || (r.extra as i64) > be {
                flag = format!("   WORSE than baseline (found {bf}, extra {be})");
                worse.push(r.log.clone());
            }
        }
        println!("\n{} {} ({}){flag}", r.log, r.kind, r.source);
        let n = r.found + r.missed + r.unseen;
        let spread = r.dt_spread.map(|(a, b)| format!("({a}, {b})")).unwrap_or_else(|| "None".into());
        println!("  casts  found {}/{n}, missed {}, unseen {}, extra {}; start off by {} ms median, {spread} range",
                 r.found, r.missed, r.unseen, r.extra, r.dt.map(|d| d.to_string()).unwrap_or_else(|| "None".into()));
        for (k, v) in &r.names {
            if !v.is_empty() {
                println!("    {k}: {v}");
            }
        }
        if let Some((a, b)) = r.swaps {
            println!("  swaps  log {a}, live {b}");
        }
        println!("  judged {}", if r.same { "the same" } else { "DIFFERENT" });
        if !r.same {
            println!("    log  {}", r.judged_log);
            println!("    live {}", r.judged_live);
        }
    }
    let g = |k: &str| tot.get(&k).copied().unwrap_or(0);
    let n = g("found") + g("missed") + g("unseen");
    println!("\n{} pulls: casts found {}/{n}, missed {}, unseen {}, extra {}; judged the same on {}/{}",
             g("pulls"), g("found"), g("missed"), g("unseen"), g("extra"), g("same"), g("pulls"));
    for (k, v) in &skills {
        if !v.is_empty() {
            println!("  {k} most: {}", most_common(v).into_iter().take(6).map(|(n, c)| format!("{n} x{c}")).collect::<Vec<_>>().join(", "));
        }
    }
    if save {
        std::fs::write(baseline(), dumps(&Value::Object(out), 1)).map_err(|e| e.to_string())?;
        println!("baseline saved: {}", baseline().display());
    } else if !worse.is_empty() {
        println!("worse than the baseline or failed: {}", worse.join(", "));
        return Ok(1);
    }
    Ok(0)
}
