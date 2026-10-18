//! Helpers the build modules share (`core/kit.py`): the bench's casts and buffs from its Elite
//! Insights JSON, `match_casts()` (yours lined up with the bench's), and the number formats.
//! Python's rounding is kept on purpose (round half to even), so both give the same numbers.

use serde_json::Value;

use crate::difflib;

pub const CUT_MS: f64 = 100.0; // shorter casts are cancelled clicks, not casts

/// A cast as the modules judge it: times in ms (fractions appear once casts are placed or ready
/// times computed). The optional fields are the keys only some casts have in the Python dicts.
#[derive(Clone, Debug, Default)]
pub struct Cast {
    pub id: i64,
    pub name: String,
    pub t: f64,
    pub dur: f64,
    /// hits up to 1 s past the cast; None for the bench and for pre-fight casts
    pub hits: Option<i64>,
    /// every hit until the skill's next cast; None where unknown
    pub all: Option<i64>,
    pub cut: bool,
    /// how the cast ended; None for a cast the log never ends, "" on the bench
    pub stop: Option<String>,
    pub cancelled: bool,
    /// None: not set (counts as true)
    pub end_ok: Option<bool>,
    /// how a pre-fight cast was placed: "end", "hit", "order" or "count"
    pub pre: Option<String>,
    /// its first hit, for a pre-fight cast placed from it
    pub hit: Option<f64>,
    pub first_hit: bool,
    pub delay: Option<f64>,
    pub ready: Option<f64>,
    pub next: Option<f64>,
    pub next_ready: Option<f64>,
}

// ---- Python's numbers ------------------------------------------------------------------------

/// round(x): half to even.
pub fn round0(x: f64) -> f64 {
    x.round_ties_even()
}

/// round(x, n): the nearest n-decimal number to the exact value, half to even (as Python).
pub fn round_n(x: f64, n: usize) -> f64 {
    format!("{x:.n$}").parse().unwrap()
}

/// statistics.median
pub fn median(xs: &[f64]) -> f64 {
    let mut v = xs.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// A number as JSON: whole numbers as integers (what the readers of the Python JSON expect).
pub fn num(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9.0e15 {
        Value::from(x as i64)
    } else {
        Value::from(x)
    }
}

pub fn onum(x: Option<f64>) -> Value {
    x.map(num).unwrap_or(Value::Null)
}

// ---- numbers as shown ------------------------------------------------------------------------

/// +0.12 / −0.05 / ±0.00 seconds; "" for None.
pub fn sgn(ms: Option<f64>) -> String {
    let Some(ms) = ms else { return String::new() };
    let s = if ms > 0.0 { "+" } else if ms < 0.0 { "−" } else { "±" };
    format!("{s}{:.2}", ms.abs() / 1000.0)
}

/// 0.68 s; — for None.
pub fn sec(ms: Option<f64>) -> String {
    match ms {
        None => "—".into(),
        Some(ms) => format!("{:.2} s", ms / 1000.0),
    }
}

/// 1:05.3 from the fight start; — for None.
pub fn clk(ms: Option<f64>) -> String {
    let Some(ms) = ms else { return "—".into() };
    let s = ms.abs() / 1000.0;
    format!("{}{}:{:04.1}", if ms < 0.0 { "−" } else { "" }, (s / 60.0).floor() as i64, s % 60.0)
}

/// 0 neutral, 1 ahead (green), 2 behind (red): a difference of more than 50 ms counts.
pub fn tone(ms: Option<f64>) -> i64 {
    match ms {
        None => 0,
        Some(ms) if ms > 50.0 => 2,
        Some(ms) if ms < -50.0 => 1,
        _ => 0,
    }
}

// ---- the bench --------------------------------------------------------------------------------

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Every cast of the log's first player, in time order, from `players[0].rotation`; zero-length
/// entries left out. EI marks a cast cancelled before it did anything with timeGained = -duration.
pub fn bench_casts(log: &Value, cut_ms: f64) -> Vec<Cast> {
    let pl = &log["players"][0];
    let sm = &log["skillMap"];
    let mut casts = vec![];
    for r in pl["rotation"].as_array().into_iter().flatten() {
        let id = r["id"].as_i64().unwrap_or(0);
        for s in r["skills"].as_array().into_iter().flatten() {
            let dur = f(&s["duration"]);
            if dur <= 0.0 {
                continue;
            }
            let gained = s.get("timeGained").map(f).unwrap_or(0.0);
            let cancelled = gained < 0.0 && -gained >= dur;
            let name = sm[format!("s{id}")]["name"].as_str().map(String::from).unwrap_or_else(|| id.to_string());
            casts.push(Cast { id, name, t: f(&s["castTime"]), dur, cut: dur < cut_ms || cancelled,
                              stop: Some(if cancelled { "cancel" } else { "" }.into()), ..Default::default() });
        }
    }
    casts.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap());
    casts
}

/// [(from, to)] while the first player had the buff (`buffUptimes` states), clipped to `start`.
pub fn bench_buffs(log: &Value, id: i64, start: f64) -> Vec<(f64, f64)> {
    let pl = &log["players"][0];
    let end = f(&log["durationMS"]);
    let mut out = vec![];
    for b in pl["buffUptimes"].as_array().into_iter().flatten() {
        if b["id"].as_i64() != Some(id) {
            continue;
        }
        let mut st: Vec<(f64, f64)> =
            b["states"].as_array().into_iter().flatten().map(|s| (f(&s[0]), f(&s[1]))).collect();
        st.push((end, 0.0));
        out = st.windows(2).filter(|w| w[0].1 > 0.0).map(|w| (w[0].0.max(start), w[1].0)).collect();
    }
    out
}

// ---- matching ---------------------------------------------------------------------------------

/// One display item of `match_casts`: yours (index into mine) with its bench cast (index into ref)
/// or none (extra), or a bench cast you missed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Item {
    Mine(usize, Option<usize>),
    Ghost(usize),
}

/// What `match_casts` needs of a cast; each build keeps its own cast type.
pub trait CastLike {
    fn id(&self) -> i64;
    fn t(&self) -> f64;
    fn cut(&self) -> bool;
}

impl CastLike for Cast {
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

/// Your casts lined up with the bench's, cut casts left out of the matching and put back by time.
/// `ordered`: by sequence (difflib), else each of yours takes the first unused bench cast of its skill.
pub fn match_casts<C: CastLike>(mine: &[C], refs: &[C], ordered: bool) -> Vec<Item> {
    let m: Vec<usize> = (0..mine.len()).filter(|&i| !mine[i].cut()).collect();
    let r: Vec<usize> = (0..refs.len()).filter(|&i| !refs[i].cut()).collect();
    let mut out = vec![];
    if ordered {
        let a: Vec<i64> = r.iter().map(|&i| refs[i].id()).collect();
        let b: Vec<i64> = m.iter().map(|&i| mine[i].id()).collect();
        for (op, i1, i2, j1, j2) in difflib::opcodes(&a, &b) {
            if op == difflib::Op::Equal {
                out.extend((0..i2 - i1).map(|x| Item::Mine(m[j1 + x], Some(r[i1 + x]))));
            } else {
                out.extend(r[i1..i2].iter().map(|&x| Item::Ghost(x)));
                out.extend(m[j1..j2].iter().map(|&x| Item::Mine(x, None)));
            }
        }
    } else {
        let mut pool = r.clone();
        for &c in &m {
            let hit = pool.iter().position(|&x| refs[x].id() == mine[c].id());
            out.push(Item::Mine(c, hit.map(|p| pool.remove(p))));
        }
        out.extend(pool.into_iter().map(Item::Ghost));
    }
    let mut cuts: std::collections::VecDeque<usize> = (0..mine.len()).filter(|&i| mine[i].cut()).collect();
    let mut res = vec![];
    for it in out {
        if let Item::Mine(c, _) = it {
            while cuts.front().is_some_and(|&x| mine[x].t() < mine[c].t()) {
                res.push(Item::Mine(cuts.pop_front().unwrap(), None));
            }
        }
        res.push(it);
    }
    res.extend(cuts.into_iter().map(|c| Item::Mine(c, None)));
    res
}

// ---- caches -----------------------------------------------------------------------------------

/// One history row for log `p`: `make(p)` (an object, or None to leave the log out), kept in
/// `cache/<log>.json` until the log grows or `version` changes (bump it when the row changes).
pub fn summary(p: &std::path::Path, cache: &std::path::Path, version: i64,
               make: impl FnOnce(&std::path::Path) -> Option<Value>) -> Option<Value> {
    let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
    let f = cache.join(format!("{}.json", crate::rot::stem(p)));
    if let Some(d) = std::fs::read_to_string(&f).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        if d["v"].as_i64() == Some(version) && d["size"].as_u64() == Some(size) {
            return Some(d);
        }
    }
    let row = make(p)?;
    let mut d = serde_json::json!({"v": version, "size": size});
    for (k, v) in row.as_object().into_iter().flatten() {
        d[k] = v.clone();
    }
    let _ = std::fs::create_dir_all(cache);
    let _ = std::fs::write(&f, d.to_string());
    Some(d)
}

/// {skill id as text: icon url}: the bench's `skillMap` icons, minus `drop`, plus the `fill` ids it
/// lacks from the GW2 API (skipped when offline), plus `alias` {new id: copied from id}. Kept in the
/// `cache` file; delete it to fetch again.
pub fn icons(cache: &std::path::Path, skill_map: &Value, fill: &[i64], drop: &[i64], alias: &[(i64, i64)]) -> Value {
    if let Some(v) = std::fs::read_to_string(cache).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok()) {
        return v;
    }
    let mut out = serde_json::Map::new();
    for (k, v) in skill_map.as_object().into_iter().flatten() {
        if let Some(icon) = v.get("icon").filter(|i| i.as_str().is_some_and(|s| !s.is_empty())) {
            out.insert(k[1..].to_string(), icon.clone());
        }
    }
    for d in drop {
        out.remove(&d.to_string());
    }
    let missing: Vec<i64> = fill.iter().copied().filter(|i| !out.contains_key(&i.to_string())).collect();
    if !missing.is_empty() {
        if let Ok(skills) = crate::web::skills(&missing) {
            for s in skills.as_array().into_iter().flatten() {
                out.insert(s["id"].to_string(), s.get("icon").cloned().unwrap_or_else(|| "".into()));
            }
        }
    }
    for (new, old) in alias {
        if let Some(v) = out.get(&old.to_string()).cloned() {
            out.insert(new.to_string(), v);
        }
    }
    let v = Value::Object(out);
    if let Some(d) = cache.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(cache, v.to_string());
    v
}

/// The local time as 2026-10-18 03:21:07, for files people read.
pub fn local_stamp() -> String {
    #[cfg(windows)]
    {
        // SAFETY: GetLocalTime only fills the struct
        let t = unsafe {
            let mut t = std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>();
            windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t);
            t
        };
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond)
    }
    #[cfg(not(windows))]
    {
        let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        format!("{s}")
    }
}
