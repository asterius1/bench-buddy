//! Your Power Scrapper golem pulls against the Snow Crows bench (`modules/scrapper/scrap.py`).
//!
//! The loop: the 20 s skills (16 s with alacrity) are the anchor. Each cycle holds three mini loops,
//! each opened by Electro-whirl + Shrapnel Grenade, with filler after it; the third one ends in the
//! bomb burst, and the next cycle starts at the next Electro-whirl. The rules and their measurements
//! are documented in the Python module; this is the same judging, step for step.

pub mod panel;
pub mod sheet;

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use bb_core::kit::{self, num, onum, round0, round_n, Cast, Item};
use bb_core::omap::OMap;
use bb_core::rot;

pub const PERMALINK: &str = "55xd-20260422-192633_golem";

pub const SCRAPPER: u32 = 43;
pub const EW: i64 = 30088;
pub const SHRAP: i64 = 5807;
pub const TC: i64 = 30713;
pub const FREEZE: i64 = 5809;
pub const POISON: i64 = 5806;
pub const MEDIC: i64 = 30357;
pub const BOB: i64 = 5813;
pub const MAGNET: i64 = 76530;
pub const FGYRO: i64 = 56920;
pub const BARRAGE: i64 = 5810;
pub const MINEFIELD: i64 = 6164;
pub const THROWMINE: i64 = 6161;
pub const GALVANIC: i64 = 5822;
pub const FIREBOMB: i64 = 5823;
pub const PGS: i64 = 30885;
pub const ENDO: i64 = 30307;
/// Positive Strike, Negative Bash, Equalizing Blow
pub const HAMMER: [i64; 3] = [30501, 29785, 30489];
/// the bomb burst opens the third mini loop's last section
const BURST: [i64; 3] = [MEDIC, BOB, MAGNET];
/// 20 s recharge (GW2 API)
pub const TWENTY: [i64; 6] = [FREEZE, POISON, MEDIC, BOB, MAGNET, TC];
/// the cooldowns shown per cycle
const TRACKED: [i64; 9] = [FREEZE, POISON, MEDIC, BOB, MAGNET, TC, FGYRO, BARRAGE, MINEFIELD];
const ALACRITY: i64 = 30328;
const SUPERSPEED: i64 = 5974;
const SWIFTNESS: i64 = 719;
const STABILITY: i64 = 1122;
const FERVOR: i64 = 76326;
const BUFFS: [i64; 5] = [ALACRITY, SUPERSPEED, SWIFTNESS, STABILITY, FERVOR];
const DODGE: i64 = 23275;
const CUT_MS: f64 = 100.0;
pub const SLOW_MS: f64 = 50.0;
const HIT_TAIL_MS: f64 = 4000.0; // casts this close to the end of the pull may not have landed yet
const FIRE_TICKS: usize = 4; // Fire Bomb hits 4 times per cast, 1 s apart
/// the usual pre-fight order
const OPENER: [i64; 6] = [THROWMINE, MINEFIELD, BOB, MAGNET, FIREBOMB, GALVANIC];

/// Recharge in seconds, counted from the end of the cast.
pub fn cd(id: i64) -> Option<f64> {
    Some(match id {
        TC | FREEZE | POISON | MEDIC | BOB | MAGNET => 20.0,
        FGYRO | BARRAGE => 25.0,
        MINEFIELD => 17.0,
        THROWMINE => 12.0,
        GALVANIC => 16.0,
        FIREBOMB => 8.0,
        PGS => 10.0,
        ENDO => 15.0,
        EW => 6.0,
        SHRAP => 5.0,
        _ => return None,
    })
}

/// Hits a cast lands until the skill's next cast.
pub fn hits(id: i64) -> Option<i64> {
    Some(match id {
        EW => 2,
        SHRAP | FREEZE | POISON => 3,
        BARRAGE => 6,
        FIREBOMB => 4,
        TC | MINEFIELD => 5,
        FGYRO | PGS | BOB | MAGNET | GALVANIC | THROWMINE | ENDO | 30501 | 29785 | 30489 => 1,
        _ => return None,
    })
}

/// Bombs go off a fixed fuse after they are placed.
fn fuse(id: i64) -> f64 {
    match id {
        BOB => 3000.0,
        MAGNET => 2000.0,
        GALVANIC | FIREBOMB => 1000.0,
        _ => 0.0,
    }
}

pub fn kit_of(id: i64) -> Option<&'static str> {
    Some(match id {
        SHRAP | FREEZE | POISON | BARRAGE => "grenade",
        FIREBOMB | GALVANIC | BOB | MAGNET => "bomb",
        PGS | ENDO => "mortar",
        FGYRO | MEDIC => "gyro",
        THROWMINE | MINEFIELD | 6162 | 6166 | -37 => "mine",
        EW | TC | 30501 | 29785 | 30489 => "hammer",
        _ => return None,
    })
}

fn names(id: i64) -> Option<&'static str> {
    (id == DODGE).then_some("Dodge")
}

pub type Buffs = HashMap<i64, Vec<(f64, f64)>>;

/// One side of the comparison: your pull or the bench.
pub struct Side {
    pub log: String,
    pub player: String,
    pub casts: Vec<Cast>,
    pub t0: f64,
    pub end: f64,
    pub buffs: Buffs,
    pub damage: f64,
}

#[derive(Clone)]
pub struct Mini {
    /// "core", then "fill" or "burst"
    pub sections: Vec<(&'static str, Vec<Cast>)>,
    pub times: OMap<&'static str, Option<f64>>,
    pub start: f64,
    pub len: Option<f64>,
    pub burst: bool,
}

impl Mini {
    pub fn section(&self, n: &str) -> Option<&Vec<Cast>> {
        self.sections.iter().find(|(k, _)| *k == n).map(|(_, v)| v)
    }
    fn real(&self) -> impl Iterator<Item = &Cast> {
        self.sections.iter().flat_map(|(_, v)| v.iter()).filter(|c| !c.cut)
    }
}

#[derive(Clone)]
pub struct Cycle {
    pub minis: Vec<Option<Mini>>,
    pub pre: Vec<Cast>,
    pub start: f64,
    pub len: Option<f64>,
}

impl Cycle {
    /// The cycle's real casts: the mini loops', then the pre-fight ones.
    fn real(&self) -> Vec<&Cast> {
        self.minis.iter().flatten().flat_map(|m| m.real()).chain(self.pre.iter().filter(|c| !c.cut)).collect()
    }
}

// ---- reading ---------------------------------------------------------------------------------

pub fn logs() -> Vec<std::path::PathBuf> {
    rot::golem_logs(SCRAPPER, &[EW, SHRAP])
}

/// On intervals of a buff from apply and all-stacks-gone times.
fn intervals(applies: &[i64], gones: &[i64], start: f64, end: f64, initial: bool) -> Vec<(f64, f64)> {
    let mut ev: Vec<(i64, u8)> = applies.iter().map(|&t| (t, 1)).chain(gones.iter().map(|&t| (t, 0))).collect();
    ev.sort();
    let mut out = vec![];
    let mut since = if initial { Some(start) } else { None };
    for (t, kind) in ev {
        let t = t as f64;
        if kind == 1 && since.is_none() {
            since = Some(t);
        } else if kind == 0 {
            if let Some(s) = since.take() {
                out.push((s, t));
            }
        }
    }
    if let Some(s) = since {
        out.push((s, end));
    }
    out
}

/// Your casts and buff intervals from a pull's record (ms from the log start).
pub fn read_mine(r: &rot::Record) -> Side {
    let mut casts: Vec<Cast> = r.casts.iter().map(|c| {
        let dur = c.dur.unwrap_or(c.exp) as f64;
        Cast { id: c.id, name: names(c.id).map(String::from).unwrap_or_else(|| c.name.clone()), t: c.t as f64, dur,
               hits: Some(c.hits), all: Some(c.all), cut: dur < CUT_MS, stop: c.stop.clone(), ..Default::default() }
    }).collect();
    // A hammer auto cancelled before it landed costs nothing but its time: left out of the matching.
    for c in casts.iter_mut() {
        if !c.cut && HAMMER.contains(&c.id) && c.all == Some(0) && c.stop.as_deref() != Some("done") {
            c.cut = true;
            c.cancelled = true;
        }
    }
    let end = r.last_hit.unwrap_or(r.end) as f64;
    for c in casts.iter_mut() {
        c.end_ok = Some(c.t + c.dur + HIT_TAIL_MS < end);
    }
    let mut all = precasts(&casts, &r.hits_at, &r.under_way, &r.names);
    all.extend(casts);
    let empty = vec![];
    let buffs = BUFFS.iter().map(|&b| {
        (b, intervals(r.buffs.get(&b).unwrap_or(&empty), r.buffs_lost.get(&b).unwrap_or(&empty), 0.0, end,
                      r.buffs_init.contains(&b)))
    }).collect();
    Side { log: r.file.clone(), player: r.player.clone(), casts: all, t0: 0.0, end, buffs, damage: r.damage as f64 }
}

/// Casts before the fight (the log starts at the first hit): see `_precasts` in scrap.py.
fn precasts(casts: &[Cast], hits: &OMap<i64, Vec<i64>>, stop_only: &OMap<i64, (i64, i64)>,
            skills: &HashMap<i64, String>) -> Vec<Cast> {
    let nm = |sid: i64| names(sid).map(String::from).or_else(|| skills.get(&sid).cloned()).unwrap_or_else(|| sid.to_string());
    let (mut out, mut mines) = (vec![], vec![]);
    for (&sid, &(t_end, dur)) in stop_only.iter() {
        if kit_of(sid).is_some() {
            out.push(Cast { id: sid, name: nm(sid), t: (t_end - dur) as f64, dur: dur as f64, stop: Some(String::new()),
                            pre: Some("end".into()), ..Default::default() });
        }
    }
    for (&sid, hs) in hits.iter() {
        if kit_of(sid).is_none() || stop_only.contains(&sid) || HAMMER.contains(&sid) || sid == EW {
            continue;
        }
        let mine: Vec<&Cast> = casts.iter().filter(|c| c.id == sid && !c.cut).collect();
        if !mine.is_empty() && mine[0].t <= hs[0] as f64 {
            continue;
        }
        let durs: Vec<f64> = if mine.is_empty() { vec![600.0] } else { mine.iter().map(|c| c.dur).collect() };
        let d = round0(kit::median(&durs));
        let mut item = Cast { id: sid, name: nm(sid), t: 0.0, dur: d, stop: Some(String::new()), pre: Some("hit".into()),
                              hit: Some(hs[0] as f64), ..Default::default() };
        if sid == THROWMINE || sid == MINEFIELD {
            item.pre = Some("order".into());
            mines.push(item);
            continue;
        }
        let mut first = hs[0] as f64;
        if sid == FIREBOMB {
            let early = hs.iter().filter(|&&h| h < hs[0] + 3500).count();
            if early < FIRE_TICKS {
                first = (hs[0] - 1000 * (FIRE_TICKS - early) as i64) as f64;
                item.first_hit = true; // its missing tick started the fight
            }
        }
        let mut delays = vec![];
        for (k, c) in mine.iter().enumerate() {
            let nxt = mine.get(k + 1);
            if let Some(&h) = hs.iter().find(|&&x| x as f64 >= c.t && nxt.is_none_or(|n| (x as f64) < n.t)) {
                delays.push(h as f64 - c.t);
            }
        }
        let delay = if delays.is_empty() { fuse(sid) + d } else { kit::median(&delays) };
        item.t = (first - delay).min(-d);
        item.hit = Some(first);
        item.delay = Some(round0(delay));
        out.push(item);
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    mines.sort_by_key(|c| OPENER.iter().position(|&x| x == c.id));
    let mut t = out.first().map(|c| c.t).unwrap_or(0.0);
    for c in mines.iter_mut().rev() {
        t -= c.dur;
        c.t = t;
    }
    mines.extend(out);
    let mut seq = mines;
    let n = seq.len();
    // one cast at a time: push an earlier cast back so it ends before the next one starts
    if let Some(c) = casts.iter().find(|c| !c.cut) {
        seq.push(c.clone());
    }
    for i in (0..seq.len().saturating_sub(1)).rev() {
        if seq[i].t + seq[i].dur > seq[i + 1].t {
            seq[i].t = seq[i + 1].t - seq[i].dur;
        }
    }
    seq.truncate(n);
    seq
}

/// The bench log is Elite Insights JSON: hits only as totals per skill. A bomb or mine with one hit
/// more than its casts was placed before the fight, back to back in the opener order.
fn bench_precasts(pl: &Value, casts: &[Cast]) -> Vec<Cast> {
    let mut hits: HashMap<i64, i64> = HashMap::new();
    for t in pl["totalDamageDist"][0].as_array().into_iter().flatten() {
        hits.insert(t["id"].as_i64().unwrap_or(0), t["hits"].as_i64().unwrap_or(0));
    }
    let det = pl["rotation"].as_array().into_iter().flatten()
        .filter(|r| matches!(r["id"].as_i64(), Some(-37 | 6166 | 6162)))
        .flat_map(|r| r["skills"].as_array().into_iter().flatten().map(|s| s["castTime"].as_f64().unwrap_or(0.0)))
        .fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))));
    let mut pre = vec![];
    for sid in OPENER {
        let cs: Vec<&Cast> = casts.iter().filter(|c| c.id == sid).collect();
        let per = if sid == FIREBOMB { FIRE_TICKS as i64 } else { 1 };
        let extra = if sid != MINEFIELD {
            hits.get(&sid).copied().unwrap_or(0) > per * cs.len() as i64
        } else {
            det.is_some_and(|d| cs.is_empty() || d < cs[0].t)
        };
        if extra {
            let d = if cs.is_empty() { 600.0 } else { round0(kit::median(&cs.iter().map(|c| c.dur).collect::<Vec<_>>())) };
            pre.push(Cast { id: sid, name: cs.first().map(|c| c.name.clone()).unwrap_or_else(|| sid.to_string()), t: 0.0,
                            dur: d, stop: Some(String::new()), pre: Some("count".into()), ..Default::default() });
        }
    }
    let mut t = casts.first().map(|c| c.t).unwrap_or(0.0);
    for c in pre.iter_mut().rev() {
        t -= c.dur;
        c.t = t;
    }
    pre
}

/// The bench: its side and its cycles, read once.
pub struct Bench {
    pub side: Side,
    pub rows: Vec<Cycle>,
    pub log: Value,
}

impl Bench {
    pub fn from_json(log: Value) -> Bench {
        let pl = &log["players"][0];
        let mut casts = kit::bench_casts(&log, CUT_MS);
        let mut all = bench_precasts(pl, &casts);
        all.append(&mut casts);
        let start = all.first().map(|c| c.t).unwrap_or(0.0);
        let buffs = BUFFS.iter().map(|&b| (b, kit::bench_buffs(&log, b, start))).collect();
        let side = Side { log: PERMALINK.into(), player: pl["name"].as_str().unwrap_or("").into(), casts: all, t0: 0.0,
                          end: log["durationMS"].as_f64().unwrap_or(0.0), buffs,
                          damage: pl["dpsTargets"][0][0]["damage"].as_f64().unwrap_or(0.0) };
        let rows = rows(&side);
        Bench { side, rows, log }
    }

    /// The bench log from `path`, downloaded from dps.report the first time.
    pub fn load(path: &Path) -> Result<Bench, String> {
        Ok(Bench::from_json(bb_core::web::bench_log(PERMALINK, path)?))
    }
}

// ---- cooldowns -------------------------------------------------------------------------------

fn on(iv: &[(f64, f64)], t: f64) -> bool {
    iv.iter().any(|&(a, b)| a <= t && t < b)
}

/// When a skill whose cast ended at `end` is ready: its recharge runs 25% faster under alacrity.
fn ready(end: f64, cd: f64, alac: &[(f64, f64)]) -> f64 {
    let (mut t, mut left) = (end, cd);
    let mut iv = alac.to_vec();
    iv.sort_by(|a, b| a.partial_cmp(b).unwrap());
    iv.push((1e12, 1e12));
    for (a, b) in iv {
        if b <= t {
            continue;
        }
        if a > t {
            if t + left <= a {
                return t + left;
            }
            left -= a - t;
            t = a;
        }
        let span = (b - t) * 1.25;
        if left <= span {
            return t + left / 1.25;
        }
        left -= span;
        t = b;
    }
    t + left
}

/// Each real cast gets when it was ready, the next real cast's start and when that one was ready.
fn link(casts: &mut [Cast], alac: &[(f64, f64)]) {
    let mut last_end: HashMap<i64, f64> = HashMap::new();
    let real: Vec<usize> = (0..casts.len()).filter(|&i| !casts[i].cut).collect();
    for &i in &real {
        let c = &mut casts[i];
        c.ready = None;
        if let Some(cd) = cd(c.id) {
            if let Some(&e) = last_end.get(&c.id) {
                c.ready = Some(ready(e, cd * 1000.0, alac));
            }
            last_end.insert(c.id, c.t + c.dur);
        }
    }
    for w in real.windows(2) {
        let (t, r) = (casts[w[1]].t, casts[w[1]].ready);
        casts[w[0]].next = Some(t);
        casts[w[0]].next_ready = r;
    }
}

// ---- cycles and mini loops ---------------------------------------------------------------------

/// Cycles split at every Electro-whirl; a mini loop with a bomb-burst skill opens its cycle.
fn cycles(casts: &[Cast], end: f64) -> Vec<Cycle> {
    let ews: Vec<f64> = casts.iter().filter(|c| !c.cut && c.id == EW).map(|c| c.t).collect();
    if ews.is_empty() {
        return vec![];
    }
    let mut bounds = ews.clone();
    bounds.push(end + 1.0);
    let pre: Vec<Cast> = casts.iter().filter(|c| c.t < ews[0]).cloned().collect();
    let mut minis = vec![];
    for w in bounds.windows(2) {
        let (a, b) = (w[0], w[1]);
        let cs: Vec<&Cast> = casts.iter().filter(|c| a <= c.t && c.t < b).collect();
        let rs: Vec<&Cast> = cs.iter().copied().filter(|c| !c.cut).collect();
        let i = rs.iter().position(|c| c.id == SHRAP).unwrap_or(0);
        let t_fill = rs.get(i + 1).map(|c| c.t);
        let j = rs.iter().skip(i + 1).any(|c| BURST.contains(&c.id)).then_some(i + 1);
        let t_burst = j.and_then(|j| rs.get(j)).map(|c| c.t);
        let pick = |f: &dyn Fn(f64) -> bool| cs.iter().filter(|c| f(c.t)).map(|&c| c.clone()).collect::<Vec<_>>();
        let mut secs: Vec<(&'static str, Vec<Cast>)> = vec![("core", pick(&|t| t_fill.is_none_or(|f| t < f)))];
        if let Some(tf) = t_fill {
            // Python's `t_burst or b`: a burst at 0 counts as none
            let upto = t_burst.filter(|&x| x != 0.0).unwrap_or(b);
            secs.push(("fill", pick(&|t| tf <= t && t < upto)));
        }
        if let Some(tb) = t_burst {
            secs.retain(|(n, _)| *n != "fill");
            secs.push(("burst", pick(&|t| t >= tb)));
        }
        let starts: Vec<(&'static str, f64)> = secs.iter()
            .filter(|(_, v)| v.iter().any(|c| !c.cut))
            .map(|(n, v)| (*n, v.iter().filter(|c| !c.cut).map(|c| c.t).fold(f64::INFINITY, f64::min)))
            .collect();
        let last = b > end;
        let mut times = OMap::default();
        for (k, &(n, t)) in starts.iter().enumerate() {
            let v = match starts.get(k + 1) {
                Some(&(_, nt)) => Some(nt - t),
                None => if last { None } else { Some(b - t) },
            };
            times.insert(n, v);
        }
        minis.push(Mini { sections: secs, times, start: a, len: if last { None } else { Some(b - a) },
                          burst: t_burst.is_some() });
    }
    // A cycle starts at the whirl with the bomb burst; before the first one the opener takes column 0.
    let mut groups: Vec<Vec<Option<Mini>>> = vec![];
    let mut cur: Vec<Option<Mini>> = vec![];
    for m in minis {
        if m.burst && !cur.is_empty() {
            groups.push(std::mem::take(&mut cur));
        }
        cur.push(Some(m));
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    if groups.first().is_some_and(|g| !g[0].as_ref().unwrap().burst) {
        groups[0].insert(0, None);
    }
    let starts: Vec<f64> = groups.iter().map(|ms| ms[0].as_ref().map(|m| m.start).unwrap_or(0.0)).collect();
    let firsts: Vec<f64> = groups.iter().map(|ms| ms.iter().flatten().next().map(|m| m.start).unwrap_or(0.0)).collect();
    let n = groups.len();
    groups.into_iter().enumerate().map(|(k, ms)| {
        let start = starts[k];
        // the next cycle's first mini loop is never the opener's empty column
        let nxt = if k + 1 < n { Some(firsts[k + 1]) } else { None };
        Cycle { minis: ms, pre: if k == 0 { pre.clone() } else { vec![] }, start, len: nxt.map(|x| x - start) }
    }).collect()
}

struct Cd {
    id: i64,
    name: String,
    t: f64,
    ready: Option<f64>,
    late: Option<f64>,
    ref_late: Option<Option<f64>>,
}

/// Per tracked skill in the cycle: when it was cast, when it was ready, how late.
fn cooldowns(cyc: &Cycle) -> Vec<Cd> {
    let cs = cyc.real();
    let mut out = vec![];
    for sid in TRACKED {
        let mut mine: Vec<&&Cast> = cs.iter().filter(|c| c.id == sid).collect();
        mine.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        for c in mine {
            out.push(Cd { id: sid, name: c.name.clone(), t: c.t, ready: c.ready, late: c.ready.map(|r| c.t - r),
                          ref_late: None });
        }
    }
    out.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    out
}

/// The bench cycle for cycle k: the same number; past the bench's last full cycle, two earlier.
fn bench_for(k: usize, n: usize) -> Option<usize> {
    let mut k = k;
    while k + 1 >= n && k > 2 {
        k -= 2;
    }
    (k < n).then_some(k)
}

fn ref_json(c: &Cast, rt0: f64) -> Value {
    json!({"id": c.id, "name": c.name, "dur": num(c.dur), "t": num(c.t - rt0)})
}

fn chip(it: Item, mine: &[Cast], refs: &[Cast], t0: f64, rt0: f64) -> Value {
    let (c, r) = match it {
        Item::Ghost(g) => {
            let g = &refs[g];
            return json!({"id": g.id, "name": g.name, "kind": "ghost", "refDur": num(g.dur), "refT": num(g.t - rt0)});
        }
        Item::Mine(c, r) => (&mine[c], r.map(|r| &refs[r])),
    };
    let mut out = Map::new();
    out.insert("id".into(), c.id.into());
    out.insert("name".into(), c.name.clone().into());
    out.insert("t".into(), num(c.t - t0));
    out.insert("dur".into(), num(c.dur));
    out.insert("hits".into(), c.hits.into());
    out.insert("stop".into(), c.stop.clone().into());
    out.insert("flags".into(), json!([]));
    let mut flags: Vec<&str> = vec![];
    if let Some(p) = c.pre.as_ref().filter(|p| !p.is_empty()) {
        out.insert("pre".into(), p.clone().into());
        out.insert("hit".into(), onum(c.hit));
    }
    if c.cut {
        out.insert("kind".into(), "cut".into());
        return Value::Object(out);
    }
    out.insert("kind".into(), if r.is_some() { "ok" } else { "extra" }.into());
    if let (Some(exp), Some(all)) = (hits(c.id), c.all) {
        if c.end_ok.unwrap_or(true) {
            out.insert("landed".into(), all.into());
            out.insert("expect".into(), exp.into());
            if all < exp {
                flags.push("few");
            }
        }
    }
    if let Some(r) = r {
        out.insert("ref".into(), json!({"id": r.id, "name": r.name, "dur": num(r.dur), "t": num(r.t - rt0)}));
        out.insert("delta".into(), num(c.dur - r.dur));
        out.insert("refDur".into(), num(r.dur));
    }
    if let Some(rd) = c.ready {
        out.insert("late".into(), num(c.t - rd));
        if let Some(rr) = r.and_then(|r| r.ready) {
            out.insert("refLate".into(), num(r.unwrap().t - rr));
        }
    }
    if let Some(next) = c.next {
        let end = c.t + c.dur;
        out.insert("slot".into(), num(next - c.t));
        if let Some(nr) = c.next_ready.filter(|&x| x != 0.0 && x > end) {
            out.insert("cdWait".into(), num(nr - end));
        }
        if let Some(rn) = r.and_then(|r| r.next) {
            let ref_slot = rn - r.unwrap().t;
            out.insert("refSlot".into(), num(ref_slot));
            let expect = (c.t + ref_slot).max(c.next_ready.filter(|&x| x != 0.0).unwrap_or(0.0));
            let lost = next - expect;
            out.insert("lost".into(), num(lost));
            if lost > SLOW_MS {
                flags.push("slow");
            }
        }
    }
    out.insert("flags".into(), json!(flags));
    Value::Object(out)
}

fn uptime(iv: &[(f64, f64)], a: f64, b: f64) -> Option<f64> {
    (b > a).then(|| iv.iter().map(|&(x, y)| (y.min(b) - x.max(a)).max(0.0)).sum::<f64>() / (b - a))
}

/// Object in Motion: +5% strike damage per boon among stability, swiftness and superspeed.
fn motion(buffs: &Buffs, a: f64, b: f64) -> Value {
    let (mut n, mut tot, mut cnt) = (0usize, 0.0f64, 0usize);
    let mut t = a;
    while t < b {
        let k = [STABILITY, SWIFTNESS, SUPERSPEED].iter().filter(|x| on(&buffs[x], t)).count();
        tot += 1.05f64.powf(k as f64) - 1.0;
        cnt += k;
        n += 1;
        t += 100.0;
    }
    let r = |x: f64| if n > 0 { num(round_n(x / n as f64, 2)) } else { Value::Null };
    json!({"bonus": r(100.0 * tot), "boons": r(cnt as f64)})
}

fn pct(iv: &[(f64, f64)], a: f64, b: f64) -> Value {
    uptime(iv, a, b).map(|u| num(round0(100.0 * u))).unwrap_or(Value::Null)
}

fn buffs_json(side: &Side, a: f64, b: f64) -> Value {
    let bf = &side.buffs;
    let ss = &bf[&SUPERSPEED];
    let mut gaps = vec![];
    for w in ss.windows(2) {
        let ((_, y1), (x2, _)) = (w[0], w[1]);
        if a <= y1 && y1 < b && x2 - y1 > 300.0 {
            gaps.push(json!({"from": num(y1 - side.t0), "len": num(x2 - y1)}));
        }
    }
    if let Some(&(_, y)) = ss.last() {
        if y < b - 300.0 {
            gaps.push(json!({"from": num(y - side.t0), "len": num(b - y)}));
        }
    }
    json!({"superspeed": pct(ss, a, b), "stability": pct(&bf[&STABILITY], a, b),
           "swiftness": pct(&bf[&SWIFTNESS], a, b), "alacrity": pct(&bf[&ALACRITY], a, b),
           "fervor": pct(&bf[&FERVOR], a, b), "motion": motion(bf, a, b), "ssGaps": gaps})
}

/// When Bloodstone Fervor came up in this cycle and which 20 s skills it covered.
fn fervor(side: &Side, cyc: &Cycle) -> Value {
    let a = cyc.start;
    let b = a + cyc.len.filter(|&l| l != 0.0).unwrap_or(1e9);
    let iv = &side.buffs[&FERVOR];
    let at: Vec<Value> = iv.iter().filter(|&&(x, _)| a - 2000.0 <= x && x < b - 2000.0).map(|&(x, _)| num(x - a)).collect();
    let hard: Vec<&Cast> = cyc.real().into_iter().filter(|c| TWENTY.contains(&c.id)).collect();
    let inside: Vec<&str> = hard.iter().filter(|c| on(iv, c.t)).map(|c| c.name.as_str()).collect();
    json!({"at": at, "inside": inside, "of": hard.len()})
}

fn rows(side: &Side) -> Vec<Cycle> {
    let mut casts = side.casts.clone();
    link(&mut casts, &side.buffs[&ALACRITY]);
    cycles(&casts, side.end)
}

fn mean_round(xs: &[f64]) -> Value {
    if xs.is_empty() { Value::Null } else { num(round0(xs.iter().sum::<f64>() / xs.len() as f64)) }
}

/// One pull cycle by cycle against the bench cycle of the same number; None without casts.
pub fn compare(rec: &rot::Record, bench: &Bench) -> Option<Value> {
    let me = read_mine(rec);
    if me.casts.is_empty() {
        return None;
    }
    let bs = &bench.side;
    let refs = &bench.rows;
    let mine = rows(&me);
    let (t0, rt0) = (me.t0, bs.t0);
    let mut out_rows = vec![];
    for (k, cy) in mine.iter().enumerate() {
        let rn = bench_for(k, refs.len());
        let rc = rn.map(|i| &refs[i]);
        let mut row = Map::new();
        row.insert("n".into(), k.into());
        row.insert("refN".into(), rn.into());
        row.insert("start".into(), num(cy.start - t0));
        row.insert("len".into(), onum(cy.len));
        row.insert("refLen".into(), onum(rc.and_then(|r| r.len)));
        row.insert("minis".into(), json!([]));
        let rpre: &[Cast] = match rc { Some(r) if k == 0 => &r.pre, _ => &[] };
        if !cy.pre.is_empty() || (rc.is_some_and(|r| !r.pre.is_empty()) && k == 0) {
            let items = kit::match_casts(&cy.pre, rpre, true);
            row.insert("pre".into(), json!({
                "chips": items.iter().map(|&it| chip(it, &cy.pre, rpre, t0, rt0)).collect::<Vec<_>>(),
                "ref": rpre.iter().filter(|c| !c.cut).map(|c| ref_json(c, rt0)).collect::<Vec<_>>()}));
        }
        let mut minis = vec![];
        for (i, m) in cy.minis.iter().enumerate() {
            let rm = rc.and_then(|r| r.minis.get(i)).and_then(|m| m.as_ref());
            let Some(m) = m else {
                minis.push(Value::Null);
                continue;
            };
            let mut secs = Map::new();
            let has = |n: &str| m.section(n).is_some() || rm.is_some_and(|r| r.section(n).is_some());
            for n in ["core", "fill", "burst"] {
                if !has(n) {
                    continue;
                }
                let empty = vec![];
                let cs = m.section(n).unwrap_or(&empty);
                let rcs = rm.and_then(|r| r.section(n)).unwrap_or(&empty);
                let items = kit::match_casts(cs, rcs, n == "core");
                secs.insert(n.into(), json!({
                    "chips": items.iter().map(|&it| chip(it, cs, rcs, t0, rt0)).collect::<Vec<_>>(),
                    "time": onum(m.times.get(&n).copied().flatten()),
                    "refTime": onum(rm.and_then(|r| r.times.get(&n).copied().flatten())),
                    "ref": rcs.iter().filter(|c| !c.cut).map(|c| ref_json(c, rt0)).collect::<Vec<_>>()}));
            }
            minis.push(json!({"sections": secs, "len": onum(m.len), "refLen": onum(rm.and_then(|r| r.len)),
                              "start": num(m.start - t0), "refStart": onum(rm.map(|r| r.start - rt0))}));
        }
        row.insert("minis".into(), Value::Array(minis));
        let mut cdm = cooldowns(cy);
        let mut cdr = rc.map(cooldowns).unwrap_or_default();
        for c in cdm.iter_mut() {
            if let Some(p) = cdr.iter().position(|x| x.id == c.id) {
                c.ref_late = Some(cdr.remove(p).late);
            }
            c.t -= t0;
            c.ready = c.ready.map(|r| r - t0);
        }
        row.insert("cd".into(), Value::Array(cdm.iter().map(|c| {
            let mut o = json!({"id": c.id, "name": c.name, "t": num(c.t), "ready": onum(c.ready), "late": onum(c.late)});
            if let Some(rl) = c.ref_late {
                o["refLate"] = onum(rl);
            }
            o
        }).collect()));
        row.insert("cdMissing".into(), Value::Array(cdr.iter().map(|x| json!({"id": x.id, "name": x.name})).collect()));
        let lates: Vec<f64> = cdm.iter().filter(|c| TWENTY.contains(&c.id)).filter_map(|c| c.late).collect();
        let rl: Vec<f64> = cdm.iter().filter(|c| TWENTY.contains(&c.id)).filter_map(|c| c.ref_late.flatten()).collect();
        row.insert("late20".into(), mean_round(&lates));
        row.insert("refLate20".into(), mean_round(&rl));
        row.insert("tc".into(), cdm.iter().find(|c| c.id == TC)
            .map(|tc| json!({"late": onum(tc.late), "refLate": onum(tc.ref_late.flatten())})).unwrap_or(Value::Null));
        row.insert("fervor".into(), json!({"mine": fervor(&me, cy), "ref": rc.map(|r| fervor(bs, r))}));
        let a = cy.start;
        let b = cy.len.filter(|&l| l != 0.0).map(|l| a + l).unwrap_or(me.end);
        row.insert("buffs".into(), buffs_json(&me, a, b));
        if let Some(rc) = rc {
            let ra = rc.start;
            row.insert("refBuffs".into(), buffs_json(bs, ra, rc.len.filter(|&l| l != 0.0).map(|l| ra + l).unwrap_or(bs.end)));
        }
        out_rows.push(Value::Object(row));
    }
    // lag: your Electro-whirls against the bench's, from both first casts
    let ew_m: Vec<f64> = me.casts.iter().filter(|c| c.id == EW && !c.cut).map(|c| c.t - t0).collect();
    let ew_r: Vec<f64> = bs.casts.iter().filter(|c| c.id == EW && !c.cut).map(|c| c.t - rt0).collect();
    let secs = (me.end - t0) / 1000.0;
    let bsecs = (bs.end - rt0) / 1000.0;
    Some(json!({
        "log": me.log, "player": me.player, "rows": out_rows, "secs": num(round_n(secs, 1)),
        "benchSecs": num(round_n(bsecs, 1)),
        "dps": if secs > 0.0 { num(round0(me.damage / secs)) } else { Value::Null },
        "benchDps": num(round0(bs.damage / bsecs)), "damage": num(me.damage),
        "lag": ew_m.iter().zip(&ew_r).map(|(m, r)| num(m - r)).collect::<Vec<_>>(),
        "ews": ew_m.iter().map(|&x| num(x)).collect::<Vec<_>>(), "slow": num(SLOW_MS),
        "buffs": buffs_json(&me, t0, me.end), "refBuffs": buffs_json(bs, rt0, bs.end),
        "twenty": TWENTY.iter().map(|x| x.to_string()).collect::<Vec<_>>(),
        "fervorIv": me.buffs[&FERVOR].iter().map(|&(a, b)| json!([num(a - t0), num(b - t0)])).collect::<Vec<_>>(),
    }))
}

/// One history row: a pull's length, DPS, cycles, how far behind at the end, Superspeed.
pub fn summary_row(rec: &rot::Record, bench: &Bench) -> Option<Value> {
    let x = compare(rec, bench)?;
    let rows = x["rows"].as_array()?;
    if rows.is_empty() {
        return None;
    }
    Some(json!({"log": x["log"], "secs": x["secs"], "dps": x["dps"], "cycles": rows.len(),
                "lag": x["lag"].as_array().and_then(|l| l.last().cloned()).unwrap_or(Value::Null),
                "superspeed": x["buffs"]["superspeed"]}))
}

pub const VERSION: i64 = 1; // bump when the summary changes, to recompute the cache

/// Icons: the bench's, plus those it lacks from the GW2 API; -37 is EI's merged "Detonate".
pub fn icons(cache: &Path, bench: &Bench) -> Value {
    kit::icons(cache, &bench.log["skillMap"], &[ENDO, DODGE, 31248, 6164, 6162, 6166], &[], &[(-37, 6166)])
}

/// compare() straight from a log file.
pub fn compare_log(path: &Path, bench: &Bench) -> Result<Option<Value>, String> {
    Ok(compare(&rot::read(path)?, bench))
}
