//! Holds the judging against a saved run of itself, on every pull.
//!
//!     checker snapshot --save     keep this run's answers as the baseline (cache/snapshot/)
//!     checker snapshot            judge everything again and show what changed since
//!
//! The answers are every JSON route of both builds, compare and pull for every Berserker pull,
//! compare and panel for every Scrapper pull, and every saved live capture judged live. Numbers
//! count as the same 1e-6 apart. The baseline holds character names, so it stays in cache/ (not in
//! git). Run it before and after a change: a change that should change nothing shows nothing.
//! The first baseline was saved from the Python checker's answers, which the Rust port matched on
//! every pull (`tools/parity.py` in the history of the port).

use std::collections::HashMap;
use std::path::Path;

use bb_app::App;
use bb_core::{live, rot};
use serde_json::{json, Map, Value};

/// Differences between two JSON values as (path, before, now).
pub fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<(String, Value, Value)>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            if (x - y).abs() > 1e-6 * x.abs().max(1.0) {
                out.push((path.into(), a.clone(), b.clone()));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => diff(p, q, &format!("{path}.{k}"), out),
                    (p, q) => out.push((format!("{path}.{k}"), p.cloned().unwrap_or("<missing>".into()), q.cloned().unwrap_or("<missing>".into()))),
                }
            }
        }
        (Value::Array(x), Value::Array(y)) => {
            if x.len() != y.len() {
                out.push((format!("{path} length"), x.len().into(), y.len().into()));
            }
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(p, q, &format!("{path}[{i}]"), out);
            }
        }
        _ if a != b => out.push((path.into(), a.clone(), b.clone())),
        _ => {}
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.chars().count() < 120 { s } else { s.chars().take(117).collect::<String>() + "..." }
}

/// Every answer, by the question.
fn answers(app: &App) -> Map<String, Value> {
    let mut out = Map::new();
    let mut ask = |path: &str| {
        let (p, q) = path.split_once('?').unwrap_or((path, ""));
        let q: HashMap<String, String> = q.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.into(), v.into())).collect();
        let v = match app.api(p, &q) {
            Ok(Some(v)) => v,
            Ok(None) => json!("<no route>"),
            Err(e) => json!(format!("<error: {e}>")),
        };
        out.insert(path.to_string(), v);
    };
    for p in ["/api/latest", "/api/history", "/api/icons", "/api/modules", "/api/scrapper/latest",
              "/api/scrapper/history", "/api/scrapper/sheet", "/api/scrapper/bench", "/api/scrapper/icons"] {
        ask(p);
    }
    for p in bb_berserker::logs() {
        let s = rot::stem(&p);
        ask(&format!("/api/compare?log={s}"));
        ask(&format!("/api/pull?log={s}"));
    }
    for p in bb_scrapper::logs() {
        let s = rot::stem(&p);
        ask(&format!("/api/scrapper/compare?log={s}"));
        ask(&format!("/api/scrapper/panel?log={s}"));
    }
    for log in rot::all_golem_logs() {
        let c = live::capture_of(&log);
        let Some(cap) = std::fs::read_to_string(&c).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) else { continue };
        let v = app.live(&json!({"elite": cap["elite"], "events": cap["events"]})).unwrap_or_else(|e| json!(format!("<error: {e}>")));
        out.insert(format!("live {}", rot::stem(&log)), v);
    }
    out
}

pub fn run(app: &App, dir: &Path, save: bool) -> i32 {
    let file = dir.join("snapshot.json");
    let t = std::time::Instant::now();
    let now = answers(app);
    println!("{} answers in {:.1} s", now.len(), t.elapsed().as_secs_f64());
    if save {
        let _ = std::fs::create_dir_all(dir);
        match std::fs::write(&file, Value::Object(now).to_string()) {
            Ok(_) => println!("saved {}", file.display()),
            Err(e) => {
                eprintln!("not saved: {e}");
                return 1;
            }
        }
        return 0;
    }
    let Some(before) = std::fs::read_to_string(&file).ok().and_then(|s| serde_json::from_str::<Map<String, Value>>(&s).ok()) else {
        println!("no baseline yet: run `checker snapshot --save` first");
        return 1;
    };
    let (mut same, mut changed, mut new) = (0, 0, 0);
    for (k, v) in &now {
        let Some(b) = before.get(k) else {
            new += 1;
            continue;
        };
        let mut d = vec![];
        diff(b, v, "", &mut d);
        if d.is_empty() {
            same += 1;
        } else {
            changed += 1;
            println!("  {k}: {} differences", d.len());
            for (p, x, y) in d.iter().take(3) {
                println!("      {p}: before {}  now {}", short(x), short(y));
            }
        }
    }
    let gone = before.keys().filter(|k| !now.contains_key(*k)).count();
    println!("{same} the same, {changed} changed, {new} new (pulls since the baseline), {gone} gone");
    (changed > 0 || gone > 0) as i32
}
