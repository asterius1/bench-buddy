//! A running pull as `rot::read()` gives a log, built from the live arcdps feed, so that a build
//! judges a live pull with the code that judges its logs.
//!
//! The live feed carries your hits, buff changes and condition ticks but no cast starts or stops and
//! no weapon swaps (addon/README.md). So casts are put back from hits: a skill's first hit lands
//! `offset` ms after its cast starts (the median over your logs), further hits of it within `window`
//! ms belong to the same cast, and for a pulsing field only those on its `pulse`. Skills that do no
//! damage show by the buff they put on you (`buff`, with a second one `gap` ms later when the first
//! alone also comes from elsewhere). A cast gets your median length of that skill (`dur`), or less
//! when the next cast starts sooner. The table is `livecasts.json`, measured by `bbtool livecast`.
//!
//! Events are your own (source = you), oldest first, as the addon keeps them:
//! {"time", "sc", "buff", "skill", "value", "buff_dmg", "result", "dst_self", "buffremove", "dst"}.
//! arcdps starts a log at the pull's first hit, so that hit is time 0 here; a cast that began before
//! it is "under_way" (as in a log, where only its end is seen) and is not among the casts.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::omap::OMap;
use crate::rot::{Cast, Record};

const PULSE_TOL: i64 = 60;
pub const TABLE: &str = include_str!("../livecasts.json");

/// How one skill's casts are put back.
#[derive(Clone, Debug)]
pub struct Rule {
    pub name: String,
    pub offset: i64,
    pub window: i64,
    pub pulse: i64,
    pub dur: i64,
    pub buff: i64,
    pub with: i64,
    pub gap: i64,
}

fn rule(v: &Value) -> Rule {
    let i = |k: &str| v[k].as_f64().map(|x| x as i64).unwrap_or(0);
    Rule { name: v["name"].as_str().unwrap_or("").into(), offset: i("offset"), window: i("window"), pulse: i("pulse"),
           dur: i("dur"), buff: i("buff"), with: i("with"), gap: i("gap") }
}

/// {skill: rule} for a build. A skill several builds cast has a rule per build ("23275/43": a
/// Scrapper's dodge) that wins over a plain one. `raw` is the table's JSON (TABLE when None).
pub fn table(elite: Option<u32>, raw: Option<&Value>) -> OMap<i64, Rule> {
    let parsed;
    let raw = match raw {
        Some(r) => r,
        None => {
            parsed = serde_json::from_str::<Value>(TABLE).unwrap_or(Value::Null);
            &parsed
        }
    };
    let (mut out, mut own) = (OMap::default(), OMap::default());
    for (k, v) in raw.as_object().into_iter().flatten() {
        let (sid, e) = k.split_once('/').map_or((k.as_str(), None), |(a, b)| (a, Some(b)));
        let Ok(sid) = sid.parse::<i64>() else { continue };
        match e {
            None => out.insert(sid, rule(v)),
            Some(e) if e.parse::<u32>().ok() == elite => own.insert(sid, rule(v)),
            _ => {}
        }
    }
    for (k, v) in own.0 {
        out.insert(k, v);
    }
    out
}

/// One of your feed events.
#[derive(Clone, Debug, Default)]
pub struct Ev {
    pub time: i64,
    pub sc: i64,
    pub buff: i64,
    pub skill: i64,
    pub value: i64,
    pub buff_dmg: i64,
    pub dst_self: bool,
    pub activation: bool,
    pub buffremove: bool,
    /// the target's address; None when the event doesn't say (older captures)
    pub dst: Option<u64>,
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::Null => false,
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

pub fn events_of(v: &Value) -> Vec<Ev> {
    let i = |e: &Value, k: &str| e[k].as_f64().map(|x| x as i64).unwrap_or(0);
    v.as_array().into_iter().flatten().map(|e| Ev {
        time: i(e, "time"), sc: i(e, "sc"), buff: i(e, "buff"), skill: i(e, "skill"), value: i(e, "value"),
        buff_dmg: i(e, "buff_dmg"), dst_self: truthy(&e["dst_self"]), activation: truthy(&e["activation"]),
        buffremove: truthy(&e["buffremove"]), dst: e.get("dst").and_then(Value::as_u64),
    }).collect()
}

/// A direct hit of yours on a target (not a condition tick, not on yourself).
pub fn direct(e: &Ev) -> bool {
    e.sc == 0 && e.buff == 0 && !e.activation && !e.buffremove && !e.dst_self && e.dst.unwrap_or(1) != 0
}

/// A cast put back: its skill, start and the times of its hits.
#[derive(Clone, Debug)]
pub struct Found {
    pub id: i64,
    pub t: i64,
    pub at: Vec<i64>,
}

/// Casts from your hits and the buffs you get, by the table, in time order.
pub fn casts_from(events: &[Ev], tab: &OMap<i64, Rule>) -> Vec<Found> {
    let mut casts: Vec<Found> = vec![];
    let mut open: HashMap<i64, Vec<(i64, usize)>> = HashMap::new();
    let mut by_buff: OMap<i64, Vec<i64>> = OMap::default();
    for (&sid, r) in tab.iter() {
        if r.buff != 0 {
            by_buff.entry(r.buff).push(sid);
        }
    }
    let mut applies: HashMap<i64, Vec<i64>> = HashMap::new();
    for e in events.iter().filter(|e| e.sc == 69 && e.dst_self) {
        applies.entry(e.skill).or_default().push(e.time);
    }
    for e in events {
        if e.sc == 69 && e.dst_self {
            if let Some(sids) = by_buff.get(&e.skill) {
                for &sid in sids {
                    let r = tab.get(&sid).unwrap();
                    // a pair: the second buff `gap` ms after the first (Superspeed comes from more than one skill)
                    if r.with != 0 && !applies.get(&r.with).is_some_and(|v| v.iter().any(|&x| (x - e.time - r.gap).abs() <= 60)) {
                        continue;
                    }
                    casts.push(Found { id: sid, t: e.time - r.offset, at: vec![] });
                    break;
                }
                continue;
            }
        }
        if !direct(e) {
            continue;
        }
        let (sid, t) = (e.skill, e.time);
        let Some(r) = tab.get(&sid) else { continue };
        if r.buff != 0 {
            continue;
        }
        let op = open.entry(sid).or_default();
        op.retain(|x| t - x.0 <= r.window);
        let p = r.pulse;
        let same = op.iter().find(|x| p == 0 || ((t - x.0 + PULSE_TOL).rem_euclid(p) - PULSE_TOL).abs() <= PULSE_TOL).map(|x| x.1);
        if let Some(i) = same {
            casts[i].at.push(t);
            continue;
        }
        op.push((t, casts.len()));
        casts.push(Found { id: sid, t: t - r.offset, at: vec![t] });
    }
    casts.sort_by_key(|c| c.t);
    casts
}

/// The live pull as `rot::read()` gives a log; None before the first hit.
pub fn record(events: &[Ev], elite: Option<u32>, tab: &OMap<i64, Rule>) -> Option<Record> {
    let first = events.iter().find(|e| direct(e))?.time;
    let mut rec = Record { elite: elite.unwrap_or(0), file: "live".into(), ..Default::default() };
    let found = casts_from(events, tab);
    for (k, c) in found.iter().enumerate() {
        let r = tab.get(&c.id).unwrap();
        // your usual length of the skill, cut by the next cast
        let mut dur = if r.dur != 0 { r.dur } else { 300 };
        let t = c.t - first;
        if let Some(n) = found.get(k + 1) {
            if 0 < n.t - c.t && n.t - c.t < dur {
                dur = n.t - c.t;
            }
        }
        if t < 0 {
            if t + dur > 0 && !rec.under_way.contains(&c.id) {
                rec.under_way.insert(c.id, (t + dur, dur));
            }
            continue;
        }
        // as rot counts them: "hits" up to 1 s past the cast's end, "during" 50 ms, "all" every one
        let end = c.t + dur;
        let n = |tail: i64| c.at.iter().filter(|&&h| h <= end + tail).count() as i64;
        rec.casts.push(Cast { id: c.id, name: r.name.clone(), t, exp: dur, base: dur, dur: Some(dur),
                              stop: Some("done".into()), hits: n(1000), during: n(50), all: c.at.len() as i64,
                              missed: 0, dmg: 0 });
    }
    let mut init: HashSet<i64> = HashSet::new();
    let mut end = 0;
    for e in events {
        let t = e.time - first;
        // what you had when the pull started: a log lists it as initial buffs (state change 18)
        if e.sc == 18 && e.dst_self {
            init.insert(e.skill);
        }
        if t < 0 {
            if e.sc == 69 && e.dst_self {
                init.insert(e.skill);
            } else if e.sc == 72 && e.dst_self {
                init.remove(&e.skill);
            }
            continue;
        }
        end = end.max(t);
        if e.sc == 0 && !e.dst_self && !e.buffremove {
            let d = if e.buff == 0 { e.value } else { e.buff_dmg };
            if d > 0 {
                rec.damage += d;
                rec.last_hit = Some(t);
                if e.buff == 0 {
                    rec.hits_at.entry(e.skill).push(t);
                }
            }
        }
        if (e.sc == 69 || e.sc == 72) && e.dst_self {
            if e.sc == 69 { &mut rec.buffs } else { &mut rec.buffs_gone }.entry(e.skill).push(t);
        }
        if e.sc == 72 {
            rec.buffs_lost.entry(e.skill).push(t);
        }
    }
    let mut init: Vec<i64> = init.into_iter().collect();
    init.sort();
    rec.buffs_init = init;
    rec.end = end;
    rec.names = tab.iter().map(|(&k, r)| (k, r.name.clone())).collect();
    Some(rec)
}

/// The live capture saved for a log: `<stem>.live.json` next to it.
pub fn capture_of(log: &Path) -> PathBuf {
    log.with_file_name(format!("{}.live.json", crate::rot::stem(log)))
}
