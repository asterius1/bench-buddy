//! A new build for Bench Buddy, starting from the template: your golem pulls against a bench log.
//!
//! Copy this folder to `crates/<your build>/` and `modules/_template/` to `modules/<your build>/`,
//! rename the package in Cargo.toml to `bb_<your build>`, fill in the settings below, and list the
//! build in `crates/app` (its Cargo.toml and `simple_builds()`). The checker then serves your page at
//! http://127.0.0.1:8771/<ROUTE>, and the addon judges it live.
//!
//! What the checker asks of a build (the `bb_app::Build` trait, see AGENTS.md):
//!     name, route        shown on the page, and the address it is served at
//!     logs()             your golem logs of this build, oldest first
//!     compare()          one pull against the bench, as JSON for page.html
//!     history()          optional: one row per pull for the picker
//!
//! This first version lines up your casts with the bench's in order (like a diff of two lists) and
//! gives each matched cast its start time against the bench's. Everything else (loops, cooldowns,
//! hits per cast, buffs) is yours to add; crates/berserker and crates/scrapper show how far it goes.
//! `bb_core::kit` has the pieces they share (reading the bench, lining up casts, cached history, icons).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bb_core::kit::{self, num, Cast, Item};
use bb_core::rot::{self, Record};
use serde_json::{json, Value};

// ---- the settings to fill in ------------------------------------------------------------------
pub const NAME: &str = "Power Scrapper (template)";
/// the page's address: http://127.0.0.1:8771/template
pub const ROUTE: &str = "template";
/// the elite spec's id in arcdps: 18 Berserker, 43 Scrapper (`bbtool whatlog` prints yours)
pub const ELITE: u32 = 43;
/// skill ids only this build casts (Electro-whirl, Shrapnel Grenade)
pub const SIGNATURE: [i64; 2] = [30088, 5807];
/// the bench log on dps.report (the part after dps.report/)
pub const PERMALINK: &str = "55xd-20260422-192633_golem";
// -------------------------------------------------------------------------------------------------

/// casts shorter than this are cancelled clicks, not casts
const CUT_MS: f64 = 100.0;

struct Bench {
    casts: Vec<Cast>,
    icons: Value,
}

pub struct Template {
    cache: PathBuf,
    bench: Mutex<Option<Arc<Bench>>>,
}

impl Template {
    /// `cache` is the build's folder for its downloads.
    pub fn new(cache: PathBuf) -> Template {
        Template { cache, bench: Mutex::new(None) }
    }

    /// The bench's casts from its Elite Insights JSON, downloaded from dps.report the first time.
    /// Times are ms from its first cast.
    fn bench(&self) -> Result<Arc<Bench>, String> {
        let mut g = self.bench.lock().unwrap_or_else(|e| e.into_inner());
        if g.is_none() {
            let log = bb_core::web::bench_log(PERMALINK, &self.cache.join("bench.json"))?;
            let mut casts: Vec<Cast> = kit::bench_casts(&log, CUT_MS).into_iter().filter(|c| !c.cut).collect();
            let t0 = casts.first().map(|c| c.t).unwrap_or(0.0);
            for c in casts.iter_mut() {
                c.t -= t0;
            }
            let icons: serde_json::Map<String, Value> = log["skillMap"].as_object().into_iter().flatten()
                .filter(|(_, v)| v["icon"].as_str().is_some_and(|s| !s.is_empty()))
                .map(|(k, v)| (k[1..].to_string(), v["icon"].clone())).collect();
            *g = Some(Arc::new(Bench { casts, icons: Value::Object(icons) }));
        }
        Ok(g.clone().unwrap())
    }
}

/// Your casts from the record, ms from your first cast.
fn mine(r: &Record) -> Vec<Cast> {
    let mut casts: Vec<Cast> = r.casts.iter().filter(|c| c.dur.unwrap_or(0) as f64 >= CUT_MS).map(|c| Cast {
        id: c.id, name: c.name.clone(), t: c.t as f64, dur: c.dur.unwrap_or(0) as f64, hits: Some(c.all), ..Default::default()
    }).collect();
    let t0 = casts.first().map(|c| c.t).unwrap_or(0.0);
    for c in casts.iter_mut() {
        c.t -= t0;
    }
    casts
}

fn cast_json(c: &Cast) -> Value {
    json!({"id": c.id, "name": c.name, "t": num(c.t), "dur": num(c.dur), "hits": c.hits, "cut": c.cut})
}

impl bb_core::build::Build for Template {
    fn name(&self) -> &str {
        NAME
    }
    fn route(&self) -> &str {
        ROUTE
    }
    fn elite(&self) -> u32 {
        ELITE
    }

    /// Your logs of this build: golem logs where a player of ELITE cast a SIGNATURE skill.
    fn logs(&self) -> Vec<PathBuf> {
        rot::golem_logs(ELITE, &SIGNATURE)
    }

    /// Your casts lined up with the bench's: each row is a matched pair, a cast the bench has and you
    /// skipped ("missed") or one you added ("extra"). "behind" is how much later than the bench you
    /// started that cast (both counted from the first cast); the summary's "behind" is the median over
    /// the matched casts, since a long diff can pair a cast with a far-off bench cast now and then.
    fn compare(&self, r: &Record, path: Option<&Path>) -> Result<Option<Value>, String> {
        let b = self.bench()?;
        let yours = mine(r);
        let end = yours.last().map(|c| c.t + 2000.0).unwrap_or(0.0);
        // the bench runs longer than a short pull
        let refs: Vec<Cast> = b.casts.iter().filter(|c| c.t <= end).cloned().collect();
        let mut rows = vec![];
        for it in kit::match_casts(&yours, &refs, true) {
            rows.push(match it {
                Item::Ghost(g) => json!({"kind": "missed", "bench": cast_json(&refs[g])}),
                Item::Mine(c, None) => json!({"kind": "extra", "you": cast_json(&yours[c])}),
                Item::Mine(c, Some(x)) => json!({"kind": "same", "bench": cast_json(&refs[x]), "you": cast_json(&yours[c]),
                                                 "behind": num(yours[c].t - refs[x].t)}),
            });
        }
        let last_you = rows.iter().rposition(|x| x.get("you").is_some());
        rows.truncate(last_you.map_or(0, |i| i + 1));
        let mut behind: Vec<f64> = rows.iter().filter(|x| x["kind"] == "same").filter_map(|x| x["behind"].as_f64()).collect();
        behind.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let count = |k: &str| rows.iter().filter(|x| x["kind"] == k).count();
        let log = path.map(rot::stem).unwrap_or_else(|| r.file.clone());
        Ok(Some(json!({"log": log, "name": NAME, "player": r.player, "bench": PERMALINK,
                       "secs": r.end as f64 / 1000.0, "casts": yours.len(), "missed": count("missed"), "extra": count("extra"),
                       "behind": behind.get(behind.len() / 2).map(|&x| num(x)), "rows": rows, "icons": b.icons})))
    }
}

#[cfg(test)]
mod tests {
    use bb_core::build::Build;

    /// `cargo test -p bb_template -- --nocapture` prints your newest pull, without the checker.
    #[test]
    fn newest_pull() {
        let t = super::Template::new(std::env::temp_dir().join("bb_template_test"));
        let Some(p) = t.logs().pop() else {
            println!("no logs of this build yet: check ELITE and SIGNATURE");
            return;
        };
        let out = t.compare(&bb_core::rot::read(&p).unwrap(), Some(&p)).unwrap().unwrap();
        println!("{} {} {:.1} s, {} casts, {} missed, {} extra", out["log"], out["player"], out["secs"].as_f64().unwrap(),
                 out["casts"], out["missed"], out["extra"]);
        for x in out["rows"].as_array().unwrap().iter().take(30) {
            let c = if x["you"].is_null() { &x["bench"] } else { &x["you"] };
            let behind = x["behind"].as_f64().map(|b| format!("{:+.2} s", b / 1000.0)).unwrap_or_default();
            println!("  {:<7} {:<28} {behind}", x["kind"].as_str().unwrap(), c["name"].as_str().unwrap());
        }
    }
}
