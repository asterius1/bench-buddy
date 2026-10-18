//! How much of a golem pull the live arcdps feeds carry, against the log of the same pull.
//!
//! ```text
//! bbtool feedcheck [SESSION_DIR]
//! ```
//!
//! SESSION_DIR is a feed recording (default the newest). Each Berserker golem log written while it
//! recorded is read raw and every event of the player is looked up in each feed by (time, kind,
//! skill, value). Arrival delay is the event's arrival minus its own time, minus the smallest such
//! difference in the session (so 0 = the fastest event seen).

use std::collections::{BTreeMap, HashMap};

use bb_core::evtc;
use bb_core::omap::OMap;

use crate::feed;
use crate::py::{median, most_common};

pub fn kind(sc: i64, buff: i64, buffremove: i64, activation: i64) -> String {
    if sc == 0 {
        return if activation != 0 { "activation" } else if buffremove != 0 { "buff remove" }
               else if buff != 0 { "condition tick" } else { "direct hit" }.into();
    }
    match sc {
        67 => "cast start".into(),
        68 => "cast end".into(),
        11 => "weapon swap".into(),
        69 => "buff apply".into(),
        70 => "buff extend".into(),
        71 => "buff active".into(),
        72 => "buff gone".into(),
        s => format!("state change {s}"),
    }
}

pub type Key = (i64, String, i64, i64);

/// {feed: {key: arrival_us}}, the first arrival of duplicates.
pub fn load_feed(path: &std::path::Path) -> BTreeMap<String, OMap<Key, i64>> {
    let mut feeds: BTreeMap<String, OMap<Key, i64>> = BTreeMap::new();
    let mut index: HashMap<(String, Key), ()> = HashMap::new();
    for j in feed::lines(path) {
        let e = &j["ev"];
        if e.is_null() || !feed::is_self(&j) {
            continue;
        }
        let i = |k: &str| e[k].as_i64().or_else(|| e[k].as_bool().map(|b| b as i64)).unwrap_or(0);
        let k: Key = (i("time"), kind(i("is_statechange"), i("buff"), i("is_buffremove"), i("is_activation")), i("skill_id"), i("value"));
        let f = j["feed"].as_str().unwrap_or("").to_string();
        if index.insert((f.clone(), k.clone()), ()).is_none() {
            feeds.entry(f).or_default().0.push((k, j["t"].as_i64().unwrap_or(0)));
        }
    }
    feeds
}

pub fn log_events(path: &std::path::Path) -> Option<Vec<(Key, String)>> {
    let log = evtc::load(path).ok()?;
    let pov = log.events.iter().find(|e| e.sc == 13).map(|e| e.src);
    let me = pov.and_then(|a| log.agent(a)).or_else(|| log.agents.iter().find(|a| a.is_player))?;
    let mut out = vec![];
    for e in log.events.iter().filter(|e| e.src == me.addr) {
        if e.sc == 0 && e.dst == 0 {
            continue;
        }
        let k = kind(e.sc as i64, e.buff as i64, e.buffremove as i64, e.activation as i64);
        let sid = e.skill as i64;
        out.push(((e.time as i64, k, sid, e.value as i64), log.skills.get(&sid).cloned().unwrap_or_else(|| sid.to_string())));
    }
    Some(out)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let sess = feed::session_arg(args.first())?;
    let feeds = load_feed(&sess.join("combat.jsonl"));
    let lookup: BTreeMap<&String, HashMap<&Key, i64>> = feeds.iter().map(|(f, m)| (f, m.iter().map(|(k, t)| (k, *t)).collect())).collect();
    let allk: Vec<&Key> = feeds.values().flat_map(|f| f.keys()).collect();
    if allk.is_empty() {
        println!("no events of yours in {}", sess.display());
        return Ok(());
    }
    let lo = allk.iter().map(|k| k.0).min().unwrap();
    let hi = allk.iter().map(|k| k.0).max().unwrap();
    let base = feeds.values().flat_map(|f| f.iter().map(|(k, t)| t - k.0 * 1000)).min().unwrap();
    let mut found: BTreeMap<&String, OMap<String, i64>> = BTreeMap::new();
    let mut total: OMap<String, i64> = OMap::default();
    let mut delay: BTreeMap<&String, Vec<f64>> = BTreeMap::new();
    let (mut casts_hit, mut first_gap, mut logs) = ([0i64, 0i64], vec![], 0);
    for p in bb_berserker::logs() {
        let Some(evs) = log_events(&p) else { continue };
        if evs.is_empty() || !(lo <= evs[0].0 .0 && evs[0].0 .0 <= hi) {
            continue;
        }
        logs += 1;
        for (k, _) in &evs {
            *total.entry_or(k.1.clone(), 0) += 1;
            for (f, m) in &lookup {
                if let Some(&t) = m.get(k) {
                    *found.entry(f).or_default().entry_or(k.1.clone(), 0) += 1;
                    if k.1 == "direct hit" {
                        delay.entry(f).or_default().push((t - base - k.0 * 1000) as f64 / 1e6);
                    }
                }
            }
        }
        let mut area_hits: HashMap<i64, Vec<i64>> = HashMap::new();
        if let Some(a) = feeds.get("area") {
            for k in a.keys().filter(|k| k.1 == "direct hit") {
                area_hits.entry(k.2).or_default().push(k.0);
            }
        }
        for (k, _) in evs.iter().filter(|(k, _)| k.1 == "cast start") {
            let (t, sid) = (k.0, k.2);
            let hs: Vec<i64> = area_hits.get(&sid).map(|v| v.iter().copied().filter(|&h| t <= h && h <= t + 3000).collect()).unwrap_or_default();
            casts_hit[1] += 1;
            if let Some(m) = hs.iter().min() {
                casts_hit[0] += 1;
                first_gap.push((m - t) as f64 / 1000.0);
            }
        }
    }
    println!("session {}: {logs} golem logs while recording\n", sess.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default());
    let names: Vec<&String> = feeds.keys().collect();
    println!("{:<16}{:>8}{}", "event kind", "in log", names.iter().map(|f| format!("{f:>10}")).collect::<String>());
    for (k, n) in most_common(&total) {
        println!("{k:<16}{n:>8}{}", names.iter().map(|f| format!("{:>10}", found.get(f).and_then(|m| m.get(&k)).copied().unwrap_or(0))).collect::<String>());
    }
    for (f, d) in delay.iter_mut() {
        d.sort_by(|a, b| a.partial_cmp(b).unwrap());
        if !d.is_empty() {
            println!("\n{f} feed, direct hits: arrival delay median {:.2} s, 90% {:.2} s, max {:.2} s (relative to the fastest event)",
                     median(d), d[(d.len() as f64 * 0.9) as usize], d[d.len() - 1]);
        }
    }
    if casts_hit[1] > 0 {
        first_gap.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!("\ncasts with a hit of the same skill in the area feed within 3 s: {} of {}{}", casts_hit[0], casts_hit[1],
                 if first_gap.is_empty() { String::new() } else { format!("; first hit {:.2} s after the cast start (median)", median(&first_gap)) });
    }
    Ok(())
}
