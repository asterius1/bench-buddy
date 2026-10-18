//! Casts from hits alone, the way the addon has to do it live.
//!
//!     bbtool livecast [SESSION_DIR]
//!
//! The live arcdps feed carries your hits but no casts (`bbtool feedcheck`). Every damaging skill's
//! first hit lands a fixed time after its cast starts, so a cast can be put back from its first hit.
//!
//! 1. From every golem log of every build it measures per skill the time from cast start to first hit
//!    (median), from first to last hit of one cast (95th percentile, the window inside which further
//!    hits belong to the same cast) and your cast length (median), and writes them to the live cast
//!    table, `core/livecasts.json`. Skills that do no damage are put back from a buff they put on you
//!    at a fixed time after the cast starts (found by `signatures()` for any build, two buffs together
//!    when one alone also comes from elsewhere, as Superspeed does).
//! 2. On a recording of the live feed it puts the casts back as the addon does and checks each
//!    against the logs of the same minutes: found (a real cast of that skill within 60 ms), wrong
//!    (none) and the real casts it never guessed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bb_core::kit::round0;
use bb_core::omap::OMap;
use bb_core::{evtc, live, rot};
use serde_json::{json, Map, Value};

use crate::feed;
use crate::py::{dumps, median, most_common};

const MATCH_MS: i64 = 60;
const MIN_CASTS: usize = 5;
/// skills that do no damage of their own show live by the buff they put on you
const BUFF: [(i64, i64); 2] = [(29940, 31708), (30189, 29466)];
const PULSE_TOL: i64 = 60;
const SIG_PRESENT: f64 = 0.9; // the buff comes after this share of the skill's casts
const SIG_PAIRED: f64 = 0.6; // the second buff of a pair, after this share
const SIG_PRECISE: f64 = 0.8; // and this share of the buff's applies (or the pair's) are such casts
const SIG_SPREAD: i64 = 150; // ms, 5th to 95th percentile of the time from cast start to the buff
const SIG_AFTER: i64 = 1500; // ms after the cast start to look for it

fn buff_of(sid: i64) -> Option<i64> {
    BUFF.iter().find(|b| b.0 == sid).map(|b| b.1)
}

fn med_round(v: &[i64]) -> i64 {
    round0(median(&v.iter().map(|&x| x as f64).collect::<Vec<_>>())) as i64
}

pub fn table_path(root: &Path) -> PathBuf {
    root.join("core").join("livecasts.json")
}

type SigKey = (i64, u32);

fn my_hits(log: &evtc::Log, pov: u64) -> HashMap<i64, Vec<i64>> {
    let mut hits: HashMap<i64, Vec<i64>> = HashMap::new();
    for e in &log.events {
        if e.src == pov && e.sc == 0 && e.buff == 0 && e.dst != 0 && e.buffremove == 0 && e.activation == 0 {
            hits.entry(e.skill as i64).or_default().push(e.time as i64);
        }
    }
    hits
}

fn measure() -> Map<String, Value> {
    let (mut off, mut span, mut gaps): (OMap<i64, Vec<i64>>, HashMap<i64, Vec<i64>>, HashMap<i64, Vec<i64>>) = Default::default();
    let mut names: HashMap<i64, String> = HashMap::new();
    let (mut boff, mut durs): (OMap<i64, Vec<i64>>, HashMap<i64, Vec<i64>>) = Default::default();
    let mut sig_starts: OMap<SigKey, Vec<(usize, i64)>> = OMap::default();
    let mut sig_applies: OMap<i64, Vec<(usize, i64)>> = OMap::default();
    let mut sig_hit: HashMap<SigKey, usize> = HashMap::new();
    let mut sig_index: HashMap<SigKey, usize> = HashMap::new();
    let mut app_index: HashMap<i64, usize> = HashMap::new();
    for (n_log, p) in rot::all_golem_logs().iter().enumerate() {
        let Ok(log) = evtc::load(p) else { continue };
        let Some(pov) = log.events.iter().find(|e| e.sc == 13).map(|e| e.src) else { continue };
        let elite = log.agent(pov).map(|a| a.elite).unwrap_or(0);
        let hits = my_hits(&log, pov);
        let mut applies: HashMap<i64, Vec<i64>> = HashMap::new();
        for e in &log.events {
            if e.sc == 69 && e.dst == pov && e.src == pov && BUFF.iter().any(|b| b.1 == e.skill as i64) {
                applies.entry(e.skill as i64).or_default().push(e.time as i64);
            }
        }
        let mut starts: OMap<i64, Vec<i64>> = OMap::default();
        let mut sidx: HashMap<i64, usize> = HashMap::new();
        for e in &log.events {
            let sid = e.skill as i64;
            if e.src == pov && e.sc == 67 {
                let i = *sidx.entry(sid).or_insert_with(|| {
                    starts.0.push((sid, vec![]));
                    starts.0.len() - 1
                });
                starts.0[i].1.push(e.time as i64);
                names.insert(sid, log.skills.get(&sid).cloned().unwrap_or_else(|| sid.to_string()));
            }
            if e.src == pov && e.sc == 68 && e.value > 0 {
                durs.entry(sid).or_default().push(e.value as i64);
            }
        }
        for e in &log.events {
            if e.sc == 69 && e.dst == pov {
                let sid = e.skill as i64;
                let i = *app_index.entry(sid).or_insert_with(|| {
                    sig_applies.0.push((sid, vec![]));
                    sig_applies.0.len() - 1
                });
                sig_applies.0[i].1.push((n_log, e.time as i64));
            }
        }
        let empty = vec![];
        for (sid, ts) in starts.iter() {
            let sid = *sid;
            for (i, &t) in ts.iter().enumerate() {
                let nxt = ts.get(i + 1).copied().unwrap_or(i64::MAX);
                let hs: Vec<i64> = hits.get(&sid).unwrap_or(&empty).iter().copied().filter(|&h| t <= h && h < nxt).collect();
                let key = (sid, elite);
                let k = *sig_index.entry(key).or_insert_with(|| {
                    sig_starts.0.push((key, vec![]));
                    sig_starts.0.len() - 1
                });
                sig_starts.0[k].1.push((n_log, t));
                *sig_hit.entry(key).or_default() += !hs.is_empty() as usize;
                if let Some(b) = buff_of(sid) {
                    if let Some(&x) = applies.get(&b).unwrap_or(&empty).iter().find(|&&x| 0 <= x - t && x - t < 2000 && x < nxt) {
                        boff.entry(sid).push(x - t);
                    }
                }
                if !hs.is_empty() {
                    off.entry(sid).push(hs[0] - t);
                    span.entry(sid).or_default().push(hs[hs.len() - 1] - hs[0]);
                    gaps.entry(sid).or_default().extend(hs[1..].iter().map(|h| h - hs[0]));
                }
            }
        }
    }
    let dur = |sid: i64| durs.get(&sid).filter(|d| !d.is_empty()).map(|d| json!(med_round(d))).unwrap_or(Value::Null);
    let mut table = Map::new();
    for (sid, o) in off.iter() {
        if o.len() < MIN_CASTS {
            continue;
        }
        let mut s = span[sid].clone();
        s.sort();
        let mut o = o.clone();
        o.sort();
        // a field that pulses (Scorched Earth every 2 s): later hits of one cast come at whole multiples
        // of the pulse after its first, so a hit off that beat is a new cast
        let empty = vec![];
        let g = gaps.get(sid).unwrap_or(&empty);
        let mut pulse = if g.len() >= MIN_CASTS {
            let mut c: OMap<i64, i64> = OMap::default();
            for &x in g {
                *c.entry_or(round0(x as f64 / 50.0) as i64 * 50, 0) += 1;
            }
            most_common(&c)[0].0
        } else {
            0
        };
        let on_beat = if pulse >= 500 {
            g.iter().filter(|&&x| ((x + PULSE_TOL).rem_euclid(pulse) - PULSE_TOL).abs() <= PULSE_TOL).count()
        } else {
            0
        };
        if (on_beat as f64) < 0.6 * g.len() as f64 {
            pulse = 0;
        }
        table.insert(sid.to_string(), json!({"name": names[sid], "offset": med_round(&o),
            "spread": o[o.len() * 19 / 20] - o[o.len() / 20], "window": s[s.len() * 19 / 20] + 150, "pulse": pulse,
            "casts": o.len(), "dur": dur(*sid)}));
    }
    for (sid, o) in boff.iter() {
        let mut o = o.clone();
        o.sort();
        table.insert(sid.to_string(), json!({"name": names[sid], "buff": buff_of(*sid), "offset": med_round(&o),
            "spread": o[o.len() * 19 / 20] - o[o.len() / 20], "window": 0, "pulse": 0, "casts": o.len(), "dur": dur(*sid)}));
    }
    // a skill several builds cast (dodge) gets one rule per build: "skill/elite"
    for ((sid, elite), rule) in signatures(&sig_starts, &sig_applies, &sig_hit).0 {
        let shared = sig_starts.keys().filter(|k| k.0 == sid).map(|k| k.1).collect::<HashSet<_>>().len() > 1;
        let key = if shared { format!("{sid}/{elite}") } else { sid.to_string() };
        if !table.contains_key(&key) {
            let mut v = Map::new();
            v.insert("name".into(), names[&sid].clone().into());
            for (k, x) in rule.as_object().unwrap() {
                v.insert(k.clone(), x.clone());
            }
            v.insert("window".into(), 0.into());
            v.insert("pulse".into(), 0.into());
            v.insert("dur".into(), dur(sid));
            table.insert(key, Value::Object(v));
        }
    }
    table
}

/// For skills that rarely hit: a buff on you at a steady time after the cast starts, rare otherwise;
/// or two together, the second one then needing to come after only SIG_PAIRED of the casts.
fn signatures(starts: &OMap<SigKey, Vec<(usize, i64)>>, applies: &OMap<i64, Vec<(usize, i64)>>,
              hit: &HashMap<SigKey, usize>) -> OMap<SigKey, Value> {
    let mut out = OMap::default();
    let mut by_log: HashMap<i64, HashMap<usize, Vec<i64>>> = HashMap::new();
    for (b, ts) in applies.iter() {
        for &(n, t) in ts {
            by_log.entry(*b).or_default().entry(n).or_default().push(t);
        }
    }
    let empty = vec![];
    for (sid, ss) in starts.iter() {
        if ss.len() < MIN_CASTS || hit.get(sid).copied().unwrap_or(0) as f64 > 0.5 * ss.len() as f64 {
            continue;
        }
        let mut at_by: HashMap<usize, Vec<i64>> = HashMap::new();
        for &(n, t) in ss {
            at_by.entry(n).or_default().push(t);
        }
        // (precision, buff, offset, spread, often)
        let mut cands: Vec<(f64, i64, i64, i64, bool)> = vec![];
        for (b, all) in applies.iter() {
            let bl = &by_log[b];
            let mut offs: Vec<i64> = ss.iter().filter_map(|&(n, t)| {
                bl.get(&n).unwrap_or(&empty).iter().map(|x| x - t).filter(|&d| 0 <= d && d <= SIG_AFTER).min()
            }).collect();
            offs.sort();
            if (offs.len() as f64) < SIG_PAIRED * ss.len() as f64 {
                continue;
            }
            let spread = offs[offs.len() * 19 / 20] - offs[offs.len() / 20];
            if spread > SIG_SPREAD {
                continue;
            }
            let (o, tol) = (med_round(&offs), spread.max(60));
            let mine = all.iter().filter(|&&(n, x)| at_by.get(&n).is_some_and(|ys| ys.iter().any(|y| (x - (y + o)).abs() <= tol))).count();
            cands.push((mine as f64 / all.len() as f64, *b, o, spread, offs.len() as f64 >= SIG_PRESENT * ss.len() as f64));
        }
        cands.sort_by(|a, b| b.partial_cmp(a).unwrap());
        if let Some(c) = cands.iter().find(|c| c.4) {
            if c.0 >= SIG_PRECISE {
                out.insert(*sid, json!({"buff": c.1, "offset": c.2, "spread": c.3, "casts": ss.len()}));
                continue;
            }
        }
        // two buffs: the first with the second `gap` ms later, which neither alone tells
        let mut best: Option<(f64, Value)> = None;
        for &(_, b1, o1, s1, often) in &cands {
            for &(_, b2, o2, _, _) in &cands {
                if b1 == b2 || o2 < o1 || !often {
                    continue;
                }
                let gap = o2 - o1;
                let b2l = &by_log[&b2];
                let pair: Vec<(usize, i64)> = applies.get(&b1).unwrap().iter().copied()
                    .filter(|&(n, x)| b2l.get(&n).unwrap_or(&empty).iter().any(|y| (y - x - gap).abs() <= 60)).collect();
                if pair.is_empty() {
                    continue;
                }
                let good = pair.iter().filter(|&&(n, x)| at_by.get(&n).is_some_and(|ys| ys.iter().any(|y| (x - (y + o1)).abs() <= s1.max(60)))).count();
                let prec = good as f64 / pair.len() as f64;
                if prec >= SIG_PRECISE && best.as_ref().is_none_or(|b| prec > b.0) {
                    best = Some((prec, json!({"buff": b1, "with": b2, "gap": gap, "offset": o1, "spread": s1, "casts": ss.len()})));
                }
            }
        }
        if let Some((_, v)) = best {
            out.insert(*sid, v);
        }
    }
    out
}

pub fn run(args: &[String], root: &Path) -> Result<(), String> {
    let table = measure();
    let path = table_path(root);
    let raw = Value::Object(table.clone());
    std::fs::write(&path, dumps(&raw, 1) + "\n").map_err(|e| e.to_string())?;
    println!("{} skills -> core\\livecasts.json", table.len());
    let sess = feed::session_arg(args.first())?;
    let events = feed::feed_events(&sess.join("combat.jsonl"));
    if events.is_empty() {
        println!("no feed events in {}", sess.display());
        return Ok(());
    }
    let (lo, hi) = (events[0]["time"].as_i64().unwrap_or(0), events[events.len() - 1]["time"].as_i64().unwrap_or(0));
    // each pull's casts put back as the addon does, against its log
    let (mut guessed, mut real): (Vec<(i64, i64)>, Vec<(i64, i64)>) = (vec![], vec![]);
    for p in rot::all_golem_logs() {
        let Ok(log) = evtc::load(&p) else { continue };
        let pov = log.events.iter().find(|e| e.sc == 13).map(|e| e.src);
        let (Some(a), Some(b)) = (log.events.first(), log.events.last()) else { continue };
        let (a, b) = (a.time as i64, b.time as i64);
        if !(lo <= a && a <= hi) {
            continue;
        }
        real.extend(log.events.iter().filter(|e| Some(e.src) == pov && e.sc == 67).map(|e| (e.time as i64, e.skill as i64)));
        let mine = live::events_of(&Value::Array(feed::pull_events(&events, a, b)));
        let elite = pov.and_then(|x| log.agent(x)).map(|x| x.elite);
        let tab = live::table(elite, Some(&raw));
        guessed.extend(live::casts_from(&mine, &tab).into_iter().filter(|c| a <= c.t && c.t <= b).map(|c| (c.t, c.id)));
    }
    let mut real_by: OMap<i64, Vec<i64>> = OMap::default();
    for &(t, s) in &real {
        real_by.entry(s).push(t);
    }
    let (mut ok, mut wrong): (OMap<i64, i64>, OMap<i64, i64>) = Default::default();
    let mut err = vec![];
    let mut hit_real: HashSet<(i64, i64)> = HashSet::new();
    for &(t, s) in &guessed {
        let best = real_by.get(&s).and_then(|v| v.iter().copied().min_by_key(|r| (r - t).abs()));
        let rule = table.get(&s.to_string()).or_else(|| table.iter().find(|(k, _)| k.starts_with(&format!("{s}/"))).map(|(_, v)| v));
        let spread = rule.and_then(|r| r["spread"].as_i64()).unwrap_or(0);
        match best {
            Some(b) if (b - t).abs() <= MATCH_MS.max(spread + 30) => {
                *ok.entry_or(s, 0) += 1;
                err.push(t - b);
                hit_real.insert((b, s));
            }
            _ => *wrong.entry_or(s, 0) += 1,
        }
    }
    let mut missed: OMap<i64, i64> = OMap::default();
    for &(t, s) in &real {
        if !hit_real.contains(&(t, s)) {
            *missed.entry_or(s, 0) += 1;
        }
    }
    let name = |s: i64| table.iter().find(|(k, _)| k.split('/').next() == Some(&s.to_string()))
        .and_then(|(_, v)| v["name"].as_str().map(String::from)).unwrap_or_else(|| s.to_string());
    let sum = |c: &OMap<i64, i64>| c.iter().map(|(_, n)| n).sum::<i64>();
    println!("\nsession {}: {} real casts, {} guessed from live hits", sess.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
             real.len(), guessed.len());
    println!("found {}, wrong {}, never guessed {}", sum(&ok), sum(&wrong), sum(&missed));
    if !err.is_empty() {
        let mut e: Vec<i64> = err.iter().map(|x| x.abs()).collect();
        e.sort();
        println!("start error median {:.0} ms, 95% {} ms", median(&e.iter().map(|&x| x as f64).collect::<Vec<_>>()), e[e.len() * 19 / 20]);
    }
    println!("\n{:<22}{:>6}{:>7}{:>7}{:>8}", "skill", "real", "found", "wrong", "missed");
    let mut order: Vec<(&i64, &Vec<i64>)> = real_by.iter().collect();
    order.sort_by_key(|(_, v)| std::cmp::Reverse(v.len()));
    let get = |c: &OMap<i64, i64>, s: i64| c.get(&s).copied().unwrap_or(0);
    for (&s, v) in order {
        let n: String = name(s).chars().take(22).collect();
        println!("{n:<22}{:>6}{:>7}{:>7}{:>8}", v.len(), get(&ok, s), get(&wrong, s), get(&missed, s));
    }
    Ok(())
}
