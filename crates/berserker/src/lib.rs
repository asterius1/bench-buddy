//! Your Condi Berserker golem pulls against the bench, for the checker and the addon.
//!
//! ```text
//! pull()      one pull: visits (swap to swap) with every cast judged, lag behind the bench
//! compare()   one pull section by section against the bench row of the same number
//! damage()    the opening seconds: damage and condition stacks against the bench
//! live()      the live record made ready for compare()
//! ```
//!
//! The loop and the rules are in modules/berserker/README.md. Ported from the Python checker step for
//! step; the port gave the same answers on every pull.

pub mod baseline;

use std::collections::HashMap;
use std::path::Path;

use baseline::{judge, names, RefSkill, Verdict, CUT_MS, STOW};
use bb_core::evtc;
use bb_core::kit::{self, num, onum, round0, round_n, CastLike, Item};
use bb_core::omap::OMap;
use bb_core::rot::{self, Record};
use serde_json::{json, Map, Value};

/// the bench: Snow Crows' Condition Berserker golem log on dps.report
pub const PERMALINK: &str = "YPVM-20260720-095643_golem";
pub const ELITE: u32 = 18;
/// Scorched Earth or Flaming Flurry: the longbow / sword build
pub const NEEDS: [i64; 2] = [29923, 30682];
pub const VERSION: i64 = 1; // bump when the summary changes, to recompute the cache

const SWORD: [i64; 9] = [14366, 29845, 80247, 80224, 29940, 30682, 14364, 14365, 14363];
const BOW: [i64; 8] = [29923, 30189, 14519, 14504, 14381, 14431, 14505, 14522];
/// a cast this much later than the bench's pace at the same point is flagged
pub const SLOW_MS: f64 = 50.0;
const SECTIONS: [&str; 5] = ["a", "b", "open", "fill", "close"];
/// the sword visit's second section starts at FoW or Flurry
const FLURRY_PART: [i64; 2] = [29940, 30682];
/// SE and BR before the closing Fan of Fire
const CLOSE: [i64; 2] = [29923, 30189];
const FOF: i64 = 14519;
/// swaps, Outrage, King of Fires, Berserk, sigils
const INSTANT: [i64; 6] = [-2, 30258, 31289, 30435, 9433, 9441];
const SE: i64 = 29923;
const BR: i64 = 30189;
/// a live swap: this long before the first cast on the other weapon (median of 286 swaps)
const SWAP_MS: f64 = 116.0;

/// Scorched Earth and Fan of Fire come back 4.0 s after the end of their cast (5 s with alacrity):
/// the shortest end-to-start recast in 81 of your SE pairs and 62 FoF pairs is 4.00 to 4.07 s.
fn cd_ms(id: i64) -> Option<f64> {
    (id == SE || id == FOF).then_some(4000.0)
}

fn weapon_of(ids: impl Iterator<Item = i64> + Clone) -> &'static str {
    if ids.clone().any(|i| SWORD.contains(&i)) {
        "sword"
    } else if ids.into_iter().any(|i| BOW.contains(&i)) {
        "bow"
    } else {
        ""
    }
}

pub fn logs() -> Vec<std::path::PathBuf> {
    rot::golem_logs(ELITE, &NEEDS)
}

// ---- the bench ----------------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct BCast {
    id: i64,
    name: String,
    t: f64,
    dur: Option<f64>,
    hits: Option<i64>,
    stop: Option<String>,
    cut: bool,
    next: Option<f64>,
    ready: Option<f64>,
    next_ready: Option<f64>,
}

impl CastLike for BCast {
    fn id(&self) -> i64 {
        self.id
    }
    fn t(&self) -> f64 {
        self.t
    }
    fn cut(&self) -> bool {
        self.cut
    }
}

#[derive(Clone, Debug)]
struct Item_ {

    sections: Vec<(&'static str, Vec<BCast>)>,
    times: OMap<&'static str, Option<f64>>,
    len: Option<f64>,
    start: f64,
}

impl Item_ {
    fn section(&self, n: &str) -> Option<&Vec<BCast>> {
        self.sections.iter().find(|(k, _)| *k == n).map(|(_, v)| v)
    }
}

#[derive(Clone, Debug, Default)]
struct Row {
    opener: Option<Item_>,
    sword: Option<Item_>,
    bow: Option<Item_>,
}

impl Row {
    fn part(&self, p: &str) -> Option<&Item_> {
        match p {
            "opener" => self.opener.as_ref(),
            "sword" => self.sword.as_ref(),
            _ => self.bow.as_ref(),
        }
    }
}

struct RefDamage {
    cum: Value,
    stacks: HashMap<&'static str, Vec<f64>>,
    buffs: Vec<(i64, String)>,
}

/// The bench, read once.
pub struct Bench {
    pub log: Value,
    rf: HashMap<i64, RefSkill>,
    swaps: Vec<f64>,
    /// the bench's first cast, its time zero
    first: f64,
    rows: Vec<Row>,
    dmg: RefDamage,
}

impl Bench {
    pub fn from_json(log: Value) -> Bench {
        let pl = &log["players"][0];
        let rot_ = pl["rotation"].as_array().cloned().unwrap_or_default();
        let skills = |r: &Value| r["skills"].as_array().cloned().unwrap_or_default();
        let ct = |s: &Value| s["castTime"].as_f64().unwrap_or(0.0);
        let first = rot_.iter().filter(|r| r["id"].as_i64() != Some(-2))
            .flat_map(|r| skills(r).into_iter().filter(|s| s["duration"].as_f64().unwrap_or(0.0) > 0.0 && ct(s) >= 0.0)
                .map(|s| ct(&s)).collect::<Vec<_>>())
            .fold(f64::INFINITY, f64::min);
        let mut swaps: Vec<f64> = rot_.iter().filter(|r| r["id"].as_i64() == Some(-2))
            .flat_map(|r| skills(r).into_iter().map(|s| ct(&s) - first).collect::<Vec<_>>()).collect();
        swaps.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let rf = baseline::reference(&log);
        // the bench's rows
        let (mut casts, mut sw) = (vec![], vec![]);
        for r in &rot_ {
            let id = r["id"].as_i64().unwrap_or(0);
            for s in skills(r) {
                let dur = s["duration"].as_f64().unwrap_or(0.0);
                if id == -2 {
                    sw.push(ct(&s));
                } else if !INSTANT.contains(&id) && dur > 0.0 {
                    let name = names(id).map(String::from)
                        .unwrap_or_else(|| log["skillMap"][format!("s{id}")]["name"].as_str().unwrap_or("").to_string());
                    casts.push(BCast { id, name, t: ct(&s), dur: Some(dur), hits: None, stop: Some(String::new()),
                                       cut: dur < CUT_MS && id != STOW, next: None, ready: None, next_ready: None });
                }
            }
        }
        casts.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
        link_next(&mut casts);
        sw.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let rows = rows_of(&casts, &sw, log["durationMS"].as_f64().unwrap_or(0.0));
        let dmg = ref_damage(&log);
        Bench { log, rf, swaps, first, rows, dmg }
    }

    pub fn load(path: &Path) -> Result<Bench, String> {
        Ok(Bench::from_json(bb_core::web::bench_log(PERMALINK, path)?))
    }

    pub fn skill_map(&self) -> &Value {
        &self.log["skillMap"]
    }
}

/// Icons: the bench's, its broken stow icon dropped, the rest filled from the GW2 API.
pub fn icons(cache: &Path, bench: &Bench) -> Value {
    kit::icons(cache, bench.skill_map(), &[14505, 14522, 14363, 14364, 14365], &[STOW], &[])
}

// ---- one pull ----------------------------------------------------------------------------------

/// One pull: visits (swap to swap) with every cast judged, lag behind the bench. None without casts.
pub fn pull(r: &Record, b: &Bench) -> Option<Value> {
    let casts = &r.casts;
    let t0 = casts.first()?.t;
    let mut bounds = vec![t0];
    bounds.extend(r.swaps.iter().map(|s| s.0).filter(|&t| t > t0));
    bounds.push(r.end);
    let (mut visits, mut fails, mut judged) = (vec![], OMap::<i64, Value>::default(), 0);
    for w in bounds.windows(2) {
        let (a, bb) = (w[0], w[1]);
        let mut cs = vec![];
        let mut ids = vec![];
        for c in casts.iter().filter(|c| a <= c.t && c.t < bb) {
            let dur = c.dur.map(|d| d as f64);
            let v = judge(c.id, c.t as f64, dur, c.hits, c.stop.as_deref(), &b.rf, &r.buffs);
            let name = names(c.id).map(String::from).unwrap_or_else(|| c.name.clone());
            if let Verdict::Checks(why) = &v {
                judged += 1;
                for w in why {
                    let f = fails.entry_or(c.id, json!({"name": name, "few": 0, "long": 0, "nobuff": 0}));
                    f[*w] = (f[*w].as_i64().unwrap_or(0) + 1).into();
                }
            }
            ids.push(c.id);
            cs.push(json!({"id": c.id, "name": name, "t": c.t - t0, "dur": c.dur, "hits": c.hits, "stop": c.stop,
                           "v": v.json(), "limit": b.rf.get(&c.id).map(|x| num(x.max + baseline::SLACK_MS)),
                           "expect": baseline::expect(c.id)}));
        }
        let weapon = weapon_of(ids.iter().copied());
        visits.push(json!({"start": a - t0, "len": bb - a, "weapon": weapon, "casts": cs, "last": bb == r.end}));
    }
    let mine_swaps: Vec<i64> = r.swaps.iter().map(|s| s.0).filter(|&t| t > t0).map(|t| t - t0).collect();
    let lag: Vec<Value> = mine_swaps.iter().zip(&b.swaps).map(|(&m, f)| num(m as f64 - f)).collect();
    let mut prev = vec![0.0];
    prev.extend(&b.swaps);
    let ref_len: Vec<Value> = prev.iter().zip(&b.swaps).map(|(a, c)| num(c - a)).collect();
    let n_fail: i64 = fails.iter().map(|(_, f)| ["few", "long", "nobuff"].iter().map(|k| f[*k].as_i64().unwrap_or(0)).sum::<i64>()).sum();
    let lens = |w: &str| -> Vec<f64> {
        visits.iter().skip(1).filter(|v| v["weapon"] == w && v["last"] == false)
            .map(|v| v["len"].as_f64().unwrap() / 1000.0).collect()
    };
    let (sw, bw) = (lens("sword"), lens("bow"));
    let fails_obj: Map<String, Value> = fails.0.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    Some(json!({"log": r.file, "secs": round_n((r.end - t0) as f64 / 1000.0, 1), "visits": visits, "lag": lag,
                "refLen": ref_len, "fails": fails_obj, "judged": judged, "failed": n_fail,
                "berserk": baseline::uptime(r, baseline::BERSERK),
                "sword": baseline::stats(&sw), "bow": baseline::stats(&bw), "player": r.player}))
}

/// One history row.
pub fn summary_row(r: &Record, b: &Bench) -> Option<Value> {
    let x = pull(r, b)?;
    let lag = x["lag"].as_array().unwrap();
    Some(json!({"log": x["log"], "secs": x["secs"],
                "loops": x["visits"].as_array().unwrap().iter().filter(|v| v["weapon"] == "sword").count(),
                "judged": x["judged"], "failed": x["failed"], "berserk": x["berserk"], "sword": x["sword"],
                "bow": x["bow"], "lag3": lag.get(5).cloned().unwrap_or(Value::Null)}))
}

// ---- section by section comparison -----------------------------------------------------------------

/// One visit's casts split into named sections. Cut casts go with the section they fall in.
fn sections(casts: &[BCast], weapon: &str, opener: bool) -> Vec<(&'static str, Vec<BCast>)> {
    let real: Vec<&BCast> = casts.iter().filter(|c| !c.cut).collect();
    let pick = |f: &dyn Fn(f64) -> bool| casts.iter().filter(|c| f(c.t)).cloned().collect::<Vec<_>>();
    if opener {
        return vec![("open", casts.to_vec())];
    }
    if weapon == "sword" {
        let i = real.iter().position(|c| FLURRY_PART.contains(&c.id)).unwrap_or(real.len());
        let tb = real.get(i).map(|c| c.t).unwrap_or(f64::INFINITY);
        return vec![("a", pick(&|t| t < tb)), ("b", pick(&|t| t >= tb))];
    }
    let ids: Vec<i64> = real.iter().map(|c| c.id).collect();
    let Some(fi) = ids.iter().position(|&x| x == FOF) else { return vec![("open", casts.to_vec())] };
    let i = fi + 1;
    let mut j = ids.len();
    if ids[ids.len() - 1] == FOF && ids.len() - 1 >= i {
        j = ids.len() - 1;
        while j >= i + 1 && CLOSE.contains(&ids[j - 1]) {
            j -= 1;
        }
    }
    let t_open = real[i - 1].t;
    let t_close = real.get(j).map(|c| c.t).unwrap_or(f64::INFINITY);
    vec![("open", pick(&|t| t <= t_open)), ("fill", pick(&|t| t_open < t && t < t_close)), ("close", pick(&|t| t >= t_close))]
}

/// Rows of sections: row 0 the opener visit, then one row per sword visit plus the longbow visit
/// after it. A section's time runs from its first cast to the next section's first cast.
fn rows_of(casts: &[BCast], swaps: &[f64], end: f64) -> Vec<Row> {
    let first = casts.first().map(|c| c.t).unwrap_or(0.0);
    let mut bounds = vec![first];
    bounds.extend(swaps.iter().copied().filter(|&t| t > first));
    bounds.push(end);
    let mut rows: Vec<Row> = vec![];
    for (k, w) in bounds.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        let cs: Vec<BCast> = casts.iter().filter(|c| a <= c.t && c.t < b).cloned().collect();
        let weapon = weapon_of(cs.iter().filter(|c| !c.cut).map(|c| c.id).collect::<Vec<_>>().into_iter());
        let last = b == end;
        let opener = k == 0 && weapon != "sword";
        let secs = sections(&cs, weapon, opener);
        let starts: Vec<(&'static str, f64)> = SECTIONS.iter().filter_map(|n| {
            let v = &secs.iter().find(|(k, _)| k == n)?.1;
            let t = v.iter().filter(|c| !c.cut).map(|c| c.t).fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x))))?;
            Some((*n, t))
        }).collect();
        let mut times = OMap::default();
        for (i, &(n, t)) in starts.iter().enumerate() {
            let v = match starts.get(i + 1) {
                Some(&(_, nt)) => Some(nt - t),
                None => if last { None } else { Some(b - t) },
            };
            times.insert(n, v);
        }
        let item = Item_ { sections: secs, times, len: if last { None } else { Some(b - a) }, start: a };
        let tail_open = rows.last().is_none_or(|r: &Row| r.bow.is_some() || r.opener.is_some());
        if opener {
            rows.push(Row { opener: Some(item), ..Default::default() });
        } else if weapon == "sword" || tail_open {
            rows.push(if weapon == "sword" { Row { sword: Some(item), ..Default::default() } }
                      else { Row { bow: Some(item), ..Default::default() } });
        } else {
            rows.last_mut().unwrap().bow = Some(item);
        }
    }
    rows
}

/// Each real cast gets the next real cast's start and when that one could have started at the
/// earliest (Scorched Earth or Fan of Fire still on cooldown; Blood Reckoning resets Scorched Earth).
fn link_next(casts: &mut [BCast]) {
    let real: Vec<usize> = (0..casts.len()).filter(|&i| !casts[i].cut).collect();
    for w in real.windows(2) {
        casts[w[0]].next = Some(casts[w[1]].t);
    }
    let mut last_end: HashMap<i64, f64> = HashMap::new();
    for &i in &real {
        let c = &mut casts[i];
        c.ready = cd_ms(c.id).and_then(|cd| last_end.get(&c.id).map(|e| e + cd));
        if c.id == BR {
            last_end.remove(&SE);
        }
        if let (Some(_), Some(d)) = (cd_ms(c.id), c.dur) {
            last_end.insert(c.id, c.t + d);
        }
    }
    for w in real.windows(2) {
        casts[w[0]].next_ready = casts[w[1]].ready;
    }
}

/// The bench row for row k: the same number; past the bench's last full row (cut by the log end),
/// the row one cycle earlier (6 loops: Flames of War every 2nd, Blood Reckoning every 3rd).
fn ref_for(k: usize, n: usize) -> Option<usize> {
    let mut k = k;
    while k + 1 >= n && k > 6 {
        k -= 6;
    }
    (k < n).then_some(k)
}

fn chip(it: Item, mine: &[BCast], refs: &[BCast], b: &Bench, buffs: &OMap<i64, Vec<i64>>, t0: f64) -> Value {
    let (c, r) = match it {
        Item::Ghost(g) => {
            let g = &refs[g];
            return json!({"id": g.id, "name": g.name, "kind": "ghost", "refDur": onum(g.dur), "refT": num(g.t - b.first)});
        }
        Item::Mine(c, r) => (&mine[c], r.map(|r| &refs[r])),
    };
    let mut out = Map::new();
    out.insert("id".into(), c.id.into());
    out.insert("name".into(), c.name.clone().into());
    out.insert("t".into(), num(c.t - t0));
    out.insert("dur".into(), onum(c.dur));
    out.insert("hits".into(), c.hits.into());
    out.insert("stop".into(), c.stop.clone().into());
    out.insert("expect".into(), baseline::expect(c.id).into());
    if c.cut {
        out.insert("kind".into(), "cut".into());
        return Value::Object(out);
    }
    let v = judge(c.id, c.t, c.dur, c.hits.unwrap_or(0), c.stop.as_deref(), &b.rf, buffs);
    let mut flags: Vec<&str> = match &v { Verdict::Checks(x) => x.iter().copied().filter(|f| *f != "long").collect(), _ => vec![] };
    out.insert("kind".into(), if r.is_some() { "ok" } else { "extra" }.into());
    if let Some(r) = r {
        out.insert("ref".into(), json!({"id": r.id, "name": r.name, "dur": onum(r.dur), "t": num(r.t - b.first)}));
    }
    let base = match r { Some(r) => r.dur, None => b.rf.get(&c.id).map(|x| x.med) };
    if let (Some(base), Some(d)) = (base, c.dur) {
        out.insert("delta".into(), num(d - base));
        out.insert("refDur".into(), num(base));
    }
    // Judged: the slot, from this cast's start to the next real cast's start, against the bench's
    // slot at the same point. Where a cast ends and its aftercast or the wait begins does not matter
    // (an auto cut short then waited out is the same as one played out); the next skill being still on
    // cooldown is not a loss, so the next cast is expected at the later of the bench's pace and that
    // skill's ready time. A closing cast before a swap is judged by when the first skill on the other
    // weapon starts.
    if let (Some(next), Some(d)) = (c.next, c.dur) {
        let end = c.t + d;
        let nr = c.next_ready.filter(|&x| x != 0.0);
        out.insert("slot".into(), num(next - c.t));
        out.insert("gap".into(), num(next - end.max(nr.unwrap_or(end))));
        if let Some(nr) = nr.filter(|&x| x > end) {
            out.insert("cdWait".into(), num(nr - end));
        }
        if let Some(rn) = r.and_then(|r| r.next) {
            let ref_slot = rn - r.unwrap().t;
            out.insert("refSlot".into(), num(ref_slot));
            let expect = (c.t + ref_slot).max(nr.unwrap_or(0.0));
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

/// The longbow visit's filler window: when SE is ready again after the opening, how late the closing
/// SE and FoF started, the filler time used and the time spent on nothing.
fn cooldown_window(it: &Item_) -> Value {
    let real = |n: &str| it.section(n).map(|v| v.iter().filter(|c| !c.cut).collect::<Vec<_>>()).unwrap_or_default();
    let (op, fill, cl) = (real("open"), real("fill"), real("close"));
    let d = |c: &BCast| c.dur.unwrap_or(0.0);
    let se_o: Vec<&&BCast> = op.iter().filter(|c| c.id == SE).collect();
    let fof_o: Vec<&&BCast> = op.iter().filter(|c| c.id == FOF).collect();
    if se_o.is_empty() || op.is_empty() {
        return Value::Null;
    }
    let w0 = op.iter().map(|c| c.t + d(c)).fold(f64::NEG_INFINITY, f64::max);
    let last_se = se_o[se_o.len() - 1];
    let se_ready = last_se.t + d(last_se) + 4000.0;
    let fof_ready = fof_o.last().map(|c| c.t + d(c) + 4000.0);
    let se_c = cl.iter().find(|c| c.id == SE);
    let fof_c = cl.iter().find(|c| c.id == FOF);
    let used: f64 = fill.iter().map(|c| d(c)).sum();
    json!({"budget": num(se_ready - w0), "used": num(used),
           "idle": se_c.map(|c| num(c.t.min(se_ready) - w0 - used)),
           "seLate": se_c.map(|c| num(c.t - se_ready)),
           "fofLate": match (fof_c, fof_ready.filter(|&x| x != 0.0)) { (Some(c), Some(fr)) => num(c.t - fr), _ => Value::Null },
           "closed": se_c.is_some()})
}

fn refchips(rcs: &[BCast]) -> Vec<Value> {
    rcs.iter().filter(|x| !x.cut).map(|x| json!({"id": x.id, "name": x.name, "dur": onum(x.dur)})).collect()
}

/// One pull section by section against the bench. `path` is the raw log, for the opening seconds'
/// damage (None for a live pull). None without casts.
pub fn compare(r: &Record, path: Option<&Path>, b: &Bench) -> Option<Value> {
    if r.casts.is_empty() {
        return None;
    }
    let mut casts: Vec<BCast> = r.casts.iter().map(|c| {
        let dur = c.dur.map(|d| d as f64);
        BCast { id: c.id, name: names(c.id).map(String::from).unwrap_or_else(|| c.name.clone()), t: c.t as f64, dur,
                hits: Some(c.hits), stop: c.stop.clone(),
                cut: judge(c.id, c.t as f64, dur, c.hits, c.stop.as_deref(), &b.rf, &r.buffs) == Verdict::Cut,
                next: None, ready: None, next_ready: None }
    }).collect();
    link_next(&mut casts);
    let t0 = casts[0].t;
    let swaps: Vec<f64> = r.swaps.iter().map(|s| s.0 as f64).collect();
    let mine = rows_of(&casts, &swaps, r.end as f64);
    let mut rows = vec![];
    let empty = vec![];
    for (k, row) in mine.iter().enumerate() {
        let rn = ref_for(k, b.rows.len());
        let rr = rn.map(|i| &b.rows[i]);
        let mut out = Map::new();
        out.insert("n".into(), k.into());
        out.insert("refN".into(), rn.into());
        for part in ["opener", "sword", "bow"] {
            let Some(mv) = row.part(part) else { continue };
            let rv = rr.and_then(|r| r.part(part));
            let mut secs = Map::new();
            for (name, cs) in &mv.sections {
                let rcs = rv.and_then(|v| v.section(name)).unwrap_or(&empty);
                let items = kit::match_casts(cs, rcs, *name != "fill");
                secs.insert(name.to_string(), json!({
                    "chips": items.iter().map(|&it| chip(it, cs, rcs, b, &r.buffs, t0)).collect::<Vec<_>>(),
                    "time": onum(mv.times.get(name).copied().flatten()),
                    "refTime": onum(rv.and_then(|v| v.times.get(name).copied().flatten())),
                    "ref": refchips(rcs)}));
            }
            for (name, rcs) in rv.map(|v| v.sections.as_slice()).unwrap_or(&[]) {
                if secs.contains_key(*name) {
                    continue;
                }
                secs.insert(name.to_string(), json!({
                    "chips": rcs.iter().filter(|x| !x.cut).map(|x| json!({"id": x.id, "name": x.name, "kind": "ghost",
                        "refDur": onum(x.dur), "refT": num(x.t - b.first)})).collect::<Vec<_>>(),
                    "time": null, "refTime": onum(rv.unwrap().times.get(name).copied().flatten()), "ref": refchips(rcs)}));
            }
            let mut o = json!({"sections": secs, "len": onum(mv.len), "refLen": onum(rv.and_then(|v| v.len)),
                               "start": num(mv.start - t0)});
            if part == "bow" {
                o["cd"] = json!({"mine": cooldown_window(mv), "ref": rv.map(cooldown_window)});
            }
            out.insert(part.into(), o);
        }
        rows.push(Value::Object(out));
    }
    let p = pull(r, b)?;
    let damage = match path { Some(p) if r.file != "live" => damage(p, b), _ => Value::Null };
    Some(json!({"log": r.file, "rows": rows, "lag": p["lag"], "secs": p["secs"], "failed": p["failed"],
                "judged": p["judged"], "berserk": p["berserk"], "sword": p["sword"], "bow": p["bow"],
                "slow": num(SLOW_MS), "damage": damage}))
}

/// The live record made ready for compare(): casts get the bench's median length, an auto attack
/// with fewer hits than a full one counts as cancelled, and the weapon swaps the feed doesn't carry
/// are put back SWAP_MS before the first cast on the other weapon.
pub fn live(mut r: Record, b: &Bench) -> Record {
    for c in r.casts.iter_mut() {
        if let Some(x) = b.rf.get(&c.id) {
            let m = round0(x.med) as i64;
            c.exp = m;
            c.base = m;
            c.dur = Some(m);
        }
        if let Some(n) = names(c.id) {
            c.name = n.into();
        }
        if baseline::AUTOS.contains(&c.id) && baseline::expect(c.id).is_some_and(|e| c.hits < e) {
            c.stop = Some("cancel".into());
        }
    }
    let mut weapon: Option<&str> = None;
    let w = |id: i64| if SWORD.contains(&id) { Some("sword") } else if BOW.contains(&id) { Some("bow") } else { None };
    let mut add = vec![];
    for p in r.casts.windows(2) {
        let wa = w(p[0].id).or(weapon);
        let wb = w(p[1].id).or(wa);
        weapon = wa;
        if let (Some(x), Some(y)) = (wa, wb) {
            if x != y {
                add.push((((p[0].t + 1) as f64).max(p[1].t as f64 - SWAP_MS)) as i64);
            }
        }
    }
    r.swaps.extend(add.into_iter().map(|t| (t, 0)));
    r.end = r.casts.iter().map(|c| c.t + c.dur.unwrap_or(0)).fold(r.end, i64::max);
    r
}

// ---- the opening seconds -------------------------------------------------------------------------

const CONDS: [(&str, &str); 4] = [("Burning", "Burning"), ("Bleeding", "Bleeding"), ("Torment", "Torment"), ("Poison", "Poisoned")];
const WINDOWS: [(f64, f64); 6] = [(0.0, 5.0), (5.0, 10.0), (10.0, 15.0), (15.0, 20.0), (20.0, 25.0), (25.0, 30.0)];
const OPENING_S: usize = 30;

fn avg_stacks(iv: &[(f64, f64)], a: f64, b: f64) -> f64 {
    round_n(kit::psum(iv.iter().map(|&(x, y)| (y.min(b) - x.max(a)).max(0.0))) / (b - a), 1)
}

fn ref_damage(log: &Value) -> RefDamage {
    let (pl, tg, bm) = (&log["players"][0], &log["targets"][0], &log["buffMap"]);
    let cum: Vec<Value> = pl["targetDamage1S"][0][0].as_array().map(|a| a.iter().take(OPENING_S + 1).cloned().collect()).unwrap_or_default();
    let mut stacks = HashMap::new();
    let pname = pl["name"].as_str().unwrap_or("");
    for (c, _) in CONDS {
        let b = tg["buffs"].as_array().into_iter().flatten().find(|x| bm[format!("b{}", x["id"])]["name"] == c);
        let mut iv = vec![];
        if let Some(b) = b {
            let mut st: Vec<(f64, i64)> = b["statesPerSource"][pname].as_array().into_iter().flatten()
                .map(|s| (s[0].as_f64().unwrap_or(0.0), s[1].as_i64().unwrap_or(0))).collect();
            st.push((1e9, 0));
            for w in st.windows(2) {
                for _ in 0..w[0].1.max(0) {
                    iv.push((w[0].0 / 1000.0, w[1].0 / 1000.0));
                }
            }
        }
        stacks.insert(c, WINDOWS.iter().map(|&(a, b)| avg_stacks(&iv, a, b)).collect());
    }
    let buffs = pl["buffUptimes"].as_array().into_iter().flatten()
        .filter_map(|b| Some((b["id"].as_i64()?, bm[format!("b{}", b["id"])]["name"].as_str()?.to_string()))).collect();
    RefDamage { cum: Value::Array(cum), stacks, buffs }
}

/// Your cumulative damage on the golem per second from your first hit, condition stacks per window,
/// and the bench's self buffs you lack.
pub fn damage(path: &Path, b: &Bench) -> Value {
    let Ok(log) = evtc::load(path) else { return Value::Null };
    let Some(me) = log.agents.iter().find(|a| a.is_player && log.by_addr.get(&a.addr).is_some_and(|&i| std::ptr::eq(&log.agents[i], *a))) else { return Value::Null };
    let golem: std::collections::HashSet<u64> = log.agents.iter().filter(|a| a.species == Some(rot::GOLEM_SPECIES)).map(|a| a.addr).collect();
    let hits: Vec<u64> = log.events.iter().filter(|e| e.sc == 0 && e.src == me.addr && golem.contains(&e.dst) && (e.value > 0 || e.buff_dmg > 0))
        .map(|e| e.time).collect();
    let Some(&first) = hits.first() else { return Value::Null };
    let mut per = vec![0i64; OPENING_S + 1];
    let mut iv: HashMap<&str, Vec<(f64, f64)>> = CONDS.iter().map(|(c, _)| (*c, vec![])).collect();
    let mut mine_buffs = std::collections::HashSet::new();
    for e in &log.events {
        if e.dst == me.addr && (e.sc == 18 || e.sc == 69) {
            mine_buffs.insert(e.skill as i64);
        }
        if e.src != me.addr || !golem.contains(&e.dst) {
            continue;
        }
        let t = (e.time as f64 - first as f64) / 1000.0;
        if e.sc == 0 {
            let d = if e.buff == 0 { e.value } else { e.buff_dmg };
            if d > 0 && (0.0..(OPENING_S + 1) as f64).contains(&t) {
                per[t as usize] += d as i64;
            }
        } else if e.sc == 69 {
            if let Some(nm) = log.skills.get(&(e.skill as i64)) {
                if let Some((c, _)) = CONDS.iter().find(|(_, n)| n == nm) {
                    iv.get_mut(c).unwrap().push((t, t + e.value as f64 / 1000.0));
                }
            }
        }
    }
    // the bench's arrays are cumulative at each second mark, ours through the end of each second
    let mut cum = vec![0i64];
    let mut tot = 0;
    for x in &per[..OPENING_S] {
        tot += x;
        cum.push(tot);
    }
    let mut stacks = Map::new();
    for (c, _) in CONDS {
        stacks.insert(c.into(), json!({"mine": WINDOWS.iter().map(|&(a, w)| avg_stacks(&iv[c], a, w)).collect::<Vec<_>>(),
                                       "ref": b.dmg.stacks[c]}));
    }
    json!({"mine": cum, "ref": b.dmg.cum, "until": (hits[hits.len() - 1] as f64 - first as f64) / 1000.0,
           "windows": WINDOWS.iter().map(|&(a, w)| json!([a as i64, w as i64])).collect::<Vec<_>>(), "stacks": stacks,
           "missingBuffs": b.dmg.buffs.iter().filter(|(i, _)| !mine_buffs.contains(i)).map(|(_, n)| n.clone()).collect::<Vec<_>>()})
}
