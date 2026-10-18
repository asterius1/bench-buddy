//! Bench Buddy's judging as one service, used the same way by the checker (the pages on
//! 127.0.0.1:8771) and by the addon (in the game, no server): `App::api()` answers the checker's
//! JSON routes, `App::live()` judges a running pull from the feed, `App::save_capture()` keeps a
//! pull's live capture next to its log.
//!
//! Builds: the Condi Berserker and the Power Scrapper have their own routes (the pages predate the
//! simple contract); any other build implements `Build` and is listed in `simple_builds()`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use bb_core::rot::{self, Record};
use bb_core::{live, omap::OMap};
use serde_json::{json, Value};

/// A build that follows the simple contract: served at /<route>, /api/<route>/latest, /history and
/// /compare?log=. `crates/template` is the starting point.
pub trait Build: Send + Sync {
    fn name(&self) -> &str;
    fn route(&self) -> &str;
    /// the elite spec id in arcdps, for live judging
    fn elite(&self) -> u32;
    /// the page, in modules/<route>/
    fn page(&self) -> &str {
        "page.html"
    }
    fn logs(&self) -> Vec<PathBuf>;
    /// one pull against the bench, as JSON for the page; `path` is None for a live pull
    fn compare(&self, rec: &Record, path: Option<&Path>) -> Result<Option<Value>, String>;
    /// one row per pull for the picker; None: just the logs
    fn history(&self) -> Option<Vec<Value>> {
        None
    }
    /// the live record adjusted before compare() (the Berserker puts back its weapon swaps)
    fn live(&self, rec: Record) -> Record {
        rec
    }
}

/// The builds of the simple contract, each with its cache folder. Add yours here.
pub fn simple_builds(_cache: &dyn Fn(&str) -> PathBuf) -> Vec<Box<dyn Build>> {
    vec![]
}

/// Where each build keeps its downloads and per-pull summaries.
#[derive(Clone)]
pub enum Caches {
    /// the repository: modules/<route>/cache (the checker)
    Repo(PathBuf),
    /// one folder: <dir>/<route> (the addon)
    Flat(PathBuf),
}

impl Caches {
    pub fn of(&self, route: &str) -> PathBuf {
        match self {
            Caches::Repo(r) => r.join("modules").join(route).join("cache"),
            Caches::Flat(d) => d.join(route),
        }
    }
}

type PullKey = (String, &'static str);

pub struct App {
    pub caches: Caches,
    bers: Mutex<Option<Arc<bb_berserker::Bench>>>,
    scrap: Mutex<Option<Arc<bb_scrapper::Bench>>>,
    pulls: Mutex<HashMap<PullKey, (u64, Value)>>,
    pub simple: Vec<Box<dyn Build>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl App {
    pub fn new(caches: Caches) -> App {
        let c = caches.clone();
        let simple = simple_builds(&move |r| c.of(r));
        App { caches, bers: Mutex::new(None), scrap: Mutex::new(None), pulls: Mutex::new(HashMap::new()), simple }
    }

    /// The Berserker bench, read (or downloaded) once.
    pub fn bers(&self) -> Result<Arc<bb_berserker::Bench>, String> {
        let mut g = lock(&self.bers);
        if g.is_none() {
            *g = Some(Arc::new(bb_berserker::Bench::load(&self.caches.of("berserker").join("log.json"))?));
        }
        Ok(g.clone().unwrap())
    }

    /// The Scrapper bench, read (or downloaded) once.
    pub fn scrap(&self) -> Result<Arc<bb_scrapper::Bench>, String> {
        let mut g = lock(&self.scrap);
        if g.is_none() {
            *g = Some(Arc::new(bb_scrapper::Bench::load(&self.caches.of("scrapper").join("bench.json"))?));
        }
        Ok(g.clone().unwrap())
    }

    /// Forget every cached result (the code changed).
    pub fn clear(&self) {
        lock(&self.pulls).clear();
    }

    /// A pull's result, kept until its log grows.
    fn pull(&self, stem: &str, what: &'static str, f: impl FnOnce(&Path) -> Result<Option<Value>, String>) -> Result<Value, String> {
        let p = rot::log_path(stem).ok_or_else(|| format!("no golem log {stem}"))?;
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let key = (stem.to_string(), what);
        if let Some((s, v)) = lock(&self.pulls).get(&key) {
            if *s == size {
                return Ok(v.clone());
            }
        }
        let v = f(&p)?.unwrap_or(Value::Null);
        lock(&self.pulls).insert(key, (size, v.clone()));
        Ok(v)
    }

    pub fn bers_compare(&self, stem: &str) -> Result<Value, String> {
        let b = self.bers()?;
        self.pull(stem, "berserker.compare", |p| Ok(bb_berserker::compare(&rot::read(p)?, Some(p), &b)))
    }

    pub fn scrap_compare(&self, stem: &str) -> Result<Value, String> {
        let b = self.scrap()?;
        self.pull(stem, "scrapper.compare", |p| Ok(bb_scrapper::compare(&rot::read(p)?, &b)))
    }

    pub fn scrap_panel(&self, stem: &str) -> Result<Value, String> {
        let x = self.scrap_compare(stem)?;
        self.pull(stem, "scrapper.panel", |_| Ok((!x.is_null()).then(|| bb_scrapper::panel::panel(x))))
    }

    fn history(&self, route: &str, version: i64, logs: Vec<PathBuf>, row: impl Fn(&Record) -> Option<Value>) -> Value {
        let cache = self.caches.of(route).join("local");
        Value::Array(logs.iter().filter_map(|p| {
            bb_core::kit::summary(p, &cache, version, |p| rot::read(p).ok().and_then(|r| row(&r)))
        }).collect())
    }

    /// Every build: its name and route.
    pub fn modules(&self) -> Value {
        let mut out = vec![json!({"name": "Condi Berserker", "route": "berserker"}), json!({"name": "Power Scrapper", "route": "scrapper"})];
        out.extend(self.simple.iter().map(|m| json!({"name": m.name(), "route": m.route()})));
        Value::Array(out)
    }

    /// The route of the build whose newest golem log is the newest of all.
    pub fn newest_route(&self) -> String {
        let mut found: Vec<(String, String)> = vec![];
        for (ls, route) in [(bb_berserker::logs(), "berserker".to_string()), (bb_scrapper::logs(), "scrapper".to_string())]
            .into_iter().chain(self.simple.iter().map(|m| (m.logs(), m.route().to_string()))) {
            if let Some(l) = ls.last() {
                found.push((rot::stem(l), route));
            }
        }
        found.into_iter().max().map(|x| x.1).unwrap_or_else(|| "berserker".into())
    }

    /// The checker's JSON routes; Ok(None) for a path that isn't one.
    pub fn api(&self, path: &str, q: &HashMap<String, String>) -> Result<Option<Value>, String> {
        let log = || q.get("log").cloned().ok_or_else(|| "no log= given".to_string());
        let latest = |ls: Vec<PathBuf>| json!({"latest": ls.last().map(|p| rot::stem(p)), "count": ls.len()});
        Ok(Some(match path {
            "/api/latest" => latest(bb_berserker::logs()),
            "/api/compare" => self.bers_compare(&log()?)?,
            "/api/pull" => {
                let b = self.bers()?;
                self.pull(&log()?, "berserker.pull", |p| Ok(bb_berserker::pull(&rot::read(p)?, &b)))?
            }
            "/api/history" => {
                let b = self.bers()?;
                self.history("berserker", bb_berserker::VERSION, bb_berserker::logs(), |r| bb_berserker::summary_row(r, &b))
            }
            "/api/icons" => bb_berserker::icons(&self.caches.of("berserker").join("icons.json"), &*self.bers()?),
            "/api/scrapper/latest" => latest(bb_scrapper::logs()),
            "/api/scrapper/history" => {
                let b = self.scrap()?;
                self.history("scrapper", bb_scrapper::VERSION, bb_scrapper::logs(), |r| bb_scrapper::summary_row(r, &b))
            }
            "/api/scrapper/compare" => self.scrap_compare(&log()?)?,
            "/api/scrapper/panel" => self.scrap_panel(&log()?)?,
            "/api/scrapper/sheet" => bb_scrapper::sheet::sheet(&*self.scrap()?),
            "/api/scrapper/bench" => bb_scrapper::sheet::bench_sheet(&*self.scrap()?),
            "/api/scrapper/icons" => bb_scrapper::icons(&self.caches.of("scrapper").join("icons.json"), &*self.scrap()?),
            "/api/modules" => self.modules(),
            _ => return self.simple_api(path, q),
        }))
    }

    fn simple_api(&self, path: &str, q: &HashMap<String, String>) -> Result<Option<Value>, String> {
        for m in &self.simple {
            let base = format!("/api/{}", m.route());
            let Some(rest) = path.strip_prefix(&base) else { continue };
            return Ok(Some(match rest {
                "/latest" => {
                    let ls = m.logs();
                    json!({"latest": ls.last().map(|p| rot::stem(p)), "count": ls.len(), "name": m.name()})
                }
                "/history" => m.history().map(Value::from)
                    .unwrap_or_else(|| m.logs().iter().map(|p| json!({"log": rot::stem(p)})).collect()),
                "/compare" => {
                    let stem = q.get("log").ok_or("no log= given")?;
                    let p = rot::log_path(stem).ok_or_else(|| format!("no golem log {stem}"))?;
                    m.compare(&rot::read(&p)?, Some(&p))?.unwrap_or(Value::Null)
                }
                _ => continue,
            }));
        }
        Ok(None)
    }

    /// Judges a running pull: `body` is {"elite", "events"} as the addon keeps them. What comes back
    /// is what is drawn for a log of that build: {"kind": "berserker", "data": compare()} or
    /// {"kind": "scrapper", "data": the panel}; another build's compare() as {"kind": its route}.
    /// Unknown elite: {"kind": null}.
    pub fn live(&self, body: &Value) -> Result<Value, String> {
        let elite = body["elite"].as_u64().map(|e| e as u32);
        let kind: &str = match elite {
            Some(bb_berserker::ELITE) => "berserker",
            Some(bb_scrapper::SCRAPPER) => "scrapper",
            _ => match self.simple.iter().find(|m| Some(m.elite()) == elite) {
                Some(m) => m.route(),
                None => return Ok(json!({"kind": null})),
            },
        };
        let tab: OMap<i64, live::Rule> = live::table(elite, None);
        let events = live::events_of(&body["events"]);
        let Some(r) = live::record(&events, elite, &tab).filter(|r| !r.casts.is_empty()) else {
            return Ok(json!({"kind": kind, "data": null}));
        };
        let data = match kind {
            "berserker" => {
                let b = self.bers()?;
                bb_berserker::compare(&bb_berserker::live(r, &b), None, &b)
            }
            "scrapper" => bb_scrapper::compare(&r, &*self.scrap()?).map(bb_scrapper::panel::panel),
            _ => {
                let m = self.simple.iter().find(|m| m.route() == kind).unwrap();
                m.compare(&m.live(r), None)?
            }
        };
        Ok(json!({"kind": kind, "data": data}))
    }

    /// Saves a pull's live capture next to its log: {"log": stem, "elite", "events", "shown"}.
    pub fn save_capture(&self, body: &Value) -> Result<Value, String> {
        let stem = body["log"].as_str().map(|s| Path::new(s).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default())
            .unwrap_or_default();
        let log = rot::log_path(&stem).filter(|_| !stem.is_empty()).ok_or_else(|| format!("no golem log {stem:?}"))?;
        let out = live::capture_of(&log);
        let mut keep = json!({});
        for k in ["log", "elite", "events", "shown"] {
            keep[k] = body.get(k).cloned().unwrap_or(Value::Null);
        }
        keep["source"] = "addon".into();
        keep["saved"] = bb_core::kit::local_stamp().into();
        std::fs::write(&out, keep.to_string()).map_err(|e| e.to_string())?;
        Ok(json!({"saved": out.display().to_string()}))
    }
}
