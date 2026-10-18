//! Per-skill expectations from the bench log, and the checks a cast of yours must pass.
//!
//! The bench is Elite Insights JSON: every cast with its duration, hits only as totals per skill.
//! A cast fails a check when
//!   - it hit fewer times than `expect()` (hits up to 1 s after the cast ends),
//!   - it took longer than the bench's longest cast of that skill plus SLACK_MS, or
//!   - it is a buff skill (`buff()`) and the buff was not applied before the cast ended.
//! Auto attacks cut short are not failures (the bench cuts them too); they are counted apart.

use std::collections::HashMap;

use bb_core::kit::{median, round_n};
use bb_core::omap::OMap;
use bb_core::rot::Record;
use serde_json::{json, Value};

pub const SLACK_MS: f64 = 100.0;
pub const CUT_MS: f64 = 100.0;
pub const AUTOS: [i64; 4] = [14364, 14365, 14363, 14431];
pub const BERSERK: i64 = 29502;
pub const STOW: i64 = 23285;

/// Hits a full cast should land within 1 s of its end, and where that number comes from: "ref" is
/// the bench's hits per cast. Flaming Flurry (30682): 47 hits in 8 casts on the bench, and the guide
/// stows after the 6th hit. Scorched Earth (29923): its first hit; the field pulses later. Combustive
/// Shot's (14505) count is from your logs.
pub fn expect(id: i64) -> Option<i64> {
    Some(match id {
        14366 | 29845 | 80247 | 80224 | 14364 | 14365 | 14363 | 29923 | 14504 | 14381 | 29613 | 30074 | 30343
        | 14505 => 1,
        30682 => 6,
        14519 => 3,
        14431 => 2,
        _ => return None,
    })
}

/// Skills that do no damage, and what they are checked by instead.
pub fn no_hit(id: i64) -> Option<&'static str> {
    match id {
        30189 => Some("applies Blood Reckoning"),
        29940 => Some("applies Flames of War (lands about 0.48 s in)"),
        _ => None,
    }
}

/// The buff a skill must apply.
pub fn buff(id: i64) -> Option<i64> {
    match id {
        29940 => Some(31708),
        30189 => Some(29466),
        _ => None,
    }
}

pub fn names(id: i64) -> Option<&'static str> {
    match id {
        80224 => Some("Rend (2nd)"),
        23285 => Some("Weapon stow"),
        14522 => Some("Combustive Shot"),
        23275 => Some("Dodge"),
        _ => None,
    }
}

/// One skill of the bench: casts, full casts' length median and range, hits.
#[derive(Clone, Debug)]
pub struct RefSkill {
    pub casts: usize,
    pub full: usize,
    pub med: f64,
    pub min: f64,
    pub max: f64,
    pub hits: i64,
}

/// Per skill from the EI JSON.
pub fn reference(log: &Value) -> HashMap<i64, RefSkill> {
    let pl = &log["players"][0];
    let mut totals: HashMap<i64, i64> = HashMap::new();
    for t in pl["totalDamageDist"][0].as_array().into_iter().flatten() {
        totals.insert(t["id"].as_i64().unwrap_or(0), t["connectedHits"].as_i64().unwrap_or(0));
    }
    let mut out = HashMap::new();
    for r in pl["rotation"].as_array().into_iter().flatten() {
        let sid = r["id"].as_i64().unwrap_or(0);
        let skills = r["skills"].as_array().map(|a| a.as_slice()).unwrap_or(&[]);
        let ds: Vec<f64> = skills.iter().map(|s| s["duration"].as_f64().unwrap_or(0.0))
            .filter(|&d| d >= CUT_MS || (sid == STOW && d > 0.0)).collect();
        if ds.is_empty() {
            continue;
        }
        out.insert(sid, RefSkill { casts: skills.len(), full: ds.len(), med: median(&ds),
                                   min: ds.iter().copied().fold(f64::INFINITY, f64::min),
                                   max: ds.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                                   hits: totals.get(&sid).copied().unwrap_or(0) });
    }
    out
}

pub fn stats(xs: &[f64]) -> Value {
    if xs.is_empty() {
        return Value::Null;
    }
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    json!({"n": v.len(), "med": round_n(median(&v), 2), "min": round_n(v[0], 2), "max": round_n(v[v.len() - 1], 2)})
}

/// The verdict on a cast: cut (an auto attack cut short or a click under CUT_MS), else the failed
/// checks ("few", "long", "nobuff"), none when the cast is fine.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Cut,
    Checks(Vec<&'static str>),
}

impl Verdict {
    pub fn json(&self) -> Value {
        match self {
            Verdict::Cut => "cut".into(),
            Verdict::Checks(v) => json!(v),
        }
    }
}

pub fn judge(id: i64, t: f64, dur: Option<f64>, hits: i64, stop: Option<&str>, rf: &HashMap<i64, RefSkill>,
             buffs: &OMap<i64, Vec<i64>>) -> Verdict {
    let dur = dur.unwrap_or(0.0);
    if dur < CUT_MS && id != STOW {
        return Verdict::Cut;
    }
    let short_auto = AUTOS.contains(&id) && stop != Some("done");
    if short_auto && expect(id).is_some_and(|e| hits < e) {
        return Verdict::Cut;
    }
    let mut out = vec![];
    if expect(id).is_some_and(|e| hits < e) {
        out.push("few");
    }
    if rf.get(&id).is_some_and(|r| dur > r.max + SLACK_MS) && !short_auto {
        out.push("long");
    }
    if let Some(b) = buff(id) {
        let empty = vec![];
        if !buffs.get(&b).unwrap_or(&empty).iter().any(|&x| t <= x as f64 && x as f64 <= t + dur + 50.0) {
            out.push("nobuff");
        }
    }
    Verdict::Checks(out)
}

/// Share of the pull from the first cast to the end with the buff on (apply 69 to all-gone 72).
pub fn uptime(r: &Record, buff: i64) -> Value {
    let Some(first) = r.casts.first() else { return Value::Null };
    let (a, b) = (first.t, r.end);
    let empty = vec![];
    let mut ev: Vec<(i64, u8)> = r.buffs.get(&buff).unwrap_or(&empty).iter().map(|&t| (t, 1))
        .chain(r.buffs_gone.get(&buff).unwrap_or(&empty).iter().map(|&t| (t, 0))).collect();
    ev.sort();
    let (mut on, mut since) = (0i64, None);
    for (t, kind) in ev {
        if kind == 1 && since.is_none() {
            since = Some(t);
        } else if kind == 0 {
            if let Some(s) = since.take() {
                on += (t.min(b) - s.max(a)).max(0);
            }
        }
    }
    if let Some(s) = since {
        on += (b - s.max(a)).max(0);
    }
    if b > a { json!(bb_core::kit::round0(100.0 * on as f64 / (b - a) as f64) as i64) } else { Value::Null }
}
