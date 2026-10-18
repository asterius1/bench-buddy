//! One player's casts and hits from a raw arcdps log: the record every module reads (`core/evtc_rot.py`).
//! Times are ms from the log's first event. The live judge will build the same record from the feed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::evtc;
use crate::omap::OMap;

pub const FIRST_ANIMATED: &str = "20261001"; // older logs carry no animation events
pub const GOLEM_SPECIES: u32 = 16199;
const LATE_MS: i64 = 50;
const WINDOW_MS: i64 = 1000;

pub fn stop_name(result: u8) -> String {
    match result {
        1 => "instant", 3 => "transition", 4 => "partial", 5 => "ended", 6 => "cancel", 7 => "stow",
        8 => "interrupt", 11 => "cc", 12 => "next skill", 13 => "motion", 14 => "dodge",
        16 => "move skill", 17 => "moved", 22 => "done",
        r => return r.to_string(),
    }
    .to_string()
}

/// hit results that did not connect: block, evade, absorb, blind
fn missed(result: u8) -> bool {
    matches!(result, 3 | 4 | 6 | 7)
}

#[derive(Clone, Debug)]
pub struct Cast {
    pub id: i64,
    pub name: String,
    pub t: i64,
    pub exp: i64,
    pub base: i64,
    pub dur: Option<i64>,
    pub stop: Option<String>,
    pub hits: i64,
    pub during: i64,
    pub all: i64,
    pub missed: i64,
    pub dmg: i64,
}

#[derive(Clone, Debug, Default)]
pub struct Record {
    pub player: String,
    pub elite: u32,
    pub casts: Vec<Cast>,
    /// (time, weapon set)
    pub swaps: Vec<(i64, i64)>,
    pub buffs: OMap<i64, Vec<i64>>,
    pub buffs_gone: OMap<i64, Vec<i64>>,
    pub loose: OMap<String, i64>,
    pub end: i64,
    pub file: String,
    pub hits_at: OMap<i64, Vec<i64>>,
    pub damage: i64,
    pub last_hit: Option<i64>,
    /// casts that only end in the log: (end, duration)
    pub under_way: OMap<i64, (i64, i64)>,
    pub buffs_init: Vec<i64>,
    pub buffs_lost: OMap<i64, Vec<i64>>,
    pub names: HashMap<i64, String>,
}

/// arcdps's log folder; BENCH_BUDDY_LOGS when it is somewhere else.
pub fn logs_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("BENCH_BUDDY_LOGS").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join("Documents").join("Guild Wars 2").join("addons").join("arcdps").join("arcdps.cbtlogs")
}

pub fn golem_dir() -> PathBuf {
    logs_dir().join("Standard Kitty Golem (16199)")
}

fn current_agents(log: &evtc::Log) -> impl Iterator<Item = &evtc::Agent> {
    log.agents.iter().enumerate().filter(move |(i, a)| log.by_addr.get(&a.addr) == Some(i)).map(|(_, a)| a)
}

/// Standard golem logs of a character of this elite spec that cast any of `needs`, oldest first.
pub fn golem_logs(elite: u32, needs: &[i64]) -> Vec<PathBuf> {
    let mut ps: Vec<PathBuf> = std::fs::read_dir(golem_dir())
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "zevtc")).collect())
        .unwrap_or_default();
    ps.sort();
    ps.into_iter()
        .filter(|p| stem(p).as_str() >= FIRST_ANIMATED)
        .filter(|p| {
            let Ok(log) = evtc::load(p) else { return false };
            let mine: HashSet<u64> = current_agents(&log).filter(|a| a.is_player && a.elite == elite).map(|a| a.addr).collect();
            !mine.is_empty()
                && log.events.iter().any(|e| e.sc == 67 && mine.contains(&e.src) && needs.contains(&(e.skill as i64)))
        })
        .collect()
}

pub fn stem(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn read(path: &Path) -> Result<Record, String> {
    let log = evtc::load(path)?;
    let events = &log.events;
    if events.is_empty() {
        return Err("no events".into());
    }
    let pov = events.iter().find(|e| e.sc == 13).map(|e| e.src);
    let me = pov.and_then(|a| log.agent(a)).or_else(|| current_agents(&log).find(|a| a.is_player)).ok_or("no player")?;
    let t0 = events[0].time as i64;
    let golem: HashSet<u64> = current_agents(&log).filter(|a| a.species == Some(GOLEM_SPECIES)).map(|a| a.addr).collect();
    let name = |sid: i64| log.skills.get(&sid).cloned().unwrap_or_else(|| sid.to_string());

    let mut rec = Record { player: me.name.clone(), elite: me.elite, file: stem(path), ..Default::default() };
    let mut open: HashMap<i64, usize> = HashMap::new();
    let mut hits: Vec<(i64, i64, i64, u8)> = vec![];
    let mut init: HashSet<i64> = HashSet::new();
    let mut started: HashSet<i64> = HashSet::new();
    for e in events {
        let sid = e.skill as i64;
        let t = e.time as i64 - t0;
        if e.dst == me.addr && (e.sc == 69 || e.sc == 72) {
            if e.sc == 69 { &mut rec.buffs } else { &mut rec.buffs_gone }.entry(sid).push(t);
        }
        if e.dst == me.addr && e.sc == 18 {
            init.insert(sid);
        }
        if e.src != me.addr {
            continue;
        }
        if e.sc == 0 && golem.contains(&e.dst) {
            if e.buff == 0 && e.value > 0 {
                rec.hits_at.entry(sid).push(t);
            }
            let d = if e.buff == 0 { e.value } else { e.buff_dmg } as i64;
            if d > 0 {
                rec.damage += d;
                rec.last_hit = Some(t);
            }
        }
        if e.sc == 72 {
            rec.buffs_lost.entry(sid).push(t);
        }
        if e.sc == 67 {
            started.insert(sid);
        } else if e.sc == 68 && !started.contains(&sid) && rec.under_way.get(&sid).is_none() {
            rec.under_way.insert(sid, (t, e.value as i64));
        }
        if e.sc == 67 {
            open.insert(sid, rec.casts.len());
            rec.casts.push(Cast { id: sid, name: name(sid), t, exp: e.value as i64,
                                  base: if e.buff_dmg != 0 { e.buff_dmg } else { e.value } as i64,
                                  dur: None, stop: None, hits: 0, during: 0, all: 0, missed: 0, dmg: 0 });
        } else if e.sc == 68 {
            if let Some(i) = open.remove(&sid) {
                rec.casts[i].dur = Some(e.value as i64);
                rec.casts[i].stop = Some(stop_name(e.result));
            }
        } else if e.sc == 11 {
            rec.swaps.push((t, e.value as i64));
        } else if e.sc == 0 && e.buff == 0 && e.dst != 0 {
            hits.push((t, sid, e.value as i64, e.result));
        }
    }
    let mut by_skill: HashMap<i64, Vec<usize>> = HashMap::new();
    for (i, c) in rec.casts.iter().enumerate() {
        by_skill.entry(c.id).or_default().push(i);
    }
    let mut loose: OMap<i64, i64> = OMap::default();
    for &(t, sid, dmg, res) in &hits {
        // the latest cast of the skill that started before the hit
        let mut at = None;
        for &i in by_skill.get(&sid).map(|v| v.as_slice()).unwrap_or(&[]) {
            if rec.casts[i].t <= t { at = Some(i) } else { break }
        }
        let Some(i) = at else {
            *loose.entry_or(sid, 0) += 1;
            continue;
        };
        let c = &mut rec.casts[i];
        c.all += 1;
        let end = c.t + c.dur.unwrap_or(c.exp);
        if t > end + WINDOW_MS {
            continue;
        }
        c.hits += 1;
        c.dmg += dmg;
        if missed(res) {
            c.missed += 1;
        }
        if t <= end + LATE_MS {
            c.during += 1;
        }
    }
    rec.loose = loose.0.into_iter().map(|(k, v)| (name(k), v)).collect();
    let mut init: Vec<i64> = init.into_iter().collect();
    init.sort();
    rec.buffs_init = init;
    rec.end = events[events.len() - 1].time as i64 - t0;
    rec.names = log.skills.clone();
    Ok(rec)
}
