//! The addon's recordings of the live feed: `<GW2>/addons/bench_buddy/sessions/<stamp>/combat.jsonl`,
//! every event of yours on both feeds ("area" = squad, "local") with its arrival time `t`
//! (microseconds since the session start).

use std::path::{Path, PathBuf};

use bb_core::omap::OMap;
use serde_json::{json, Value};

/// The game's addons folder; BENCH_BUDDY_ADDONS when the game is installed elsewhere.
pub fn addons() -> PathBuf {
    std::env::var_os("BENCH_BUDDY_ADDONS").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Guild Wars 2\addons"))
}

pub fn session_root() -> PathBuf {
    addons().join("bench_buddy").join("sessions")
}

/// Every recording, oldest first.
pub fn sessions() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(session_root()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    v.sort();
    v
}

/// The newest recording that holds a feed, by its file's time.
pub fn newest_session() -> Option<PathBuf> {
    sessions().into_iter()
        .filter_map(|d| {
            let f = d.join("combat.jsonl");
            let m = std::fs::metadata(&f).ok().filter(|m| m.len() > 0)?;
            Some((m.modified().ok()?, d))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|x| x.1)
}

/// The recording named on the command line, else the newest.
pub fn session_arg(arg: Option<&String>) -> Result<PathBuf, String> {
    match arg {
        Some(a) => Ok(PathBuf::from(a)),
        None => newest_session().ok_or_else(|| format!("no feed recordings in {}", session_root().display())),
    }
}

/// Every line of a recording, parsed.
pub fn lines(path: &Path) -> Vec<Value> {
    std::fs::read_to_string(path).unwrap_or_default().lines().filter_map(|l| serde_json::from_str(l).ok()).collect()
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::Null => false,
        _ => true,
    }
}

pub fn is_self(j: &Value) -> bool {
    j.get("src").is_some_and(|s| truthy(&s["self"]))
}

/// The area feed's events of yours, in the shape the addon keeps them. Old recordings have no
/// "dst_self": your agent is the target of your initial buffs (state change 18).
pub fn feed_events(path: &Path) -> Vec<Value> {
    let evs: Vec<Value> = lines(path).into_iter()
        .filter(|j| j["feed"] == "area" && j.get("ev").is_some_and(|e| !e.is_null()) && is_self(j))
        .map(|j| j["ev"].clone()).collect();
    let mut n: OMap<u64, i64> = OMap::default();
    for e in evs.iter().filter(|e| e["is_statechange"] == 18) {
        *n.entry_or(e["dst_agent"].as_u64().unwrap_or(0), 0) += 1;
    }
    let me = crate::py::most_common(&n).first().map(|x| x.0);
    let mut out: Vec<Value> = evs.iter().map(|e| {
        let dst_self = e.get("dst_self").cloned().unwrap_or_else(|| (e["dst_agent"].as_u64() == me).into());
        json!({"time": e["time"], "sc": e["is_statechange"], "buff": e["buff"], "skill": e["skill_id"],
               "value": e["value"], "buff_dmg": e["buff_dmg"], "result": e["result"],
               "activation": e["is_activation"], "buffremove": e["is_buffremove"], "dst": e["dst_agent"],
               "dst_self": dst_self})
    }).collect();
    out.sort_by_key(|e| e["time"].as_i64().unwrap_or(0));
    out
}

/// A pull's events: from shortly before its first hit (casts under way) to its end.
pub fn pull_events(evs: &[Value], a: i64, b: i64) -> Vec<Value> {
    evs.iter().filter(|e| e["time"].as_i64().is_some_and(|t| a - 3000 <= t && t <= b)).cloned().collect()
}
