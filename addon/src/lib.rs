//! Bench Buddy: the golem checker's panel inside the game (Condi Berserker splits, Power Scrapper grid).
//!
//! The analysis stays in the checker (`core/serve.py` on 127.0.0.1:8771): a background
//! thread asks it for the newest golem log and its report, and the panel (`panel.rs`) draws that
//! the way `modules/berserker/views/splits.html` does, for the newest pull or any earlier one picked at the
//! top. While a pull runs, its splits fill in: your casts put back from your hits on the arcdps
//! feed (`live.rs`, about 2.7 s behind) are judged by `core/live_server.py` with the checker's own
//! code; the addon starts that server when it is not running. Reads nothing from game memory;
//! writes a recording of your own feed events to `<GW2>/addons/bench_buddy/sessions/` for
//! `tools/feedcheck.py`.

mod live;
mod panel;
mod report;

use nexus::{
    event::arc::{CombatData, COMBAT_LOCAL, COMBAT_SQUAD},
    event_consume,
    gui::{register_render, render, RenderType},
    imgui::{Condition, Ui, Window},
    keybind::{keybind_handler, register_keybind_with_string},
    paths::get_addon_dir,
    AddonFlags,
};
use report::Report;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

nexus::export! {
    name: "Bench Buddy",
    signature: -0x4242_5544,  // "BBUD"; unofficial addons use a negative id
    load,
    unload,
    flags: AddonFlags::None,
}

pub const CHECKER: &str = "127.0.0.1:8771";
const JUDGE: &str = "127.0.0.1:8772";

/// One earlier pull, for the picker.
pub struct Hist {
    pub log: String,
    /// "berserker" or "scrapper"
    pub kind: &'static str,
    pub secs: f64,
    pub loops: i64,
}

pub struct State {
    /// the checker's report as fetched, kept to judge it again when the filler mode changes
    raw: Option<Value>,
    pub report: Option<Report>,
    pub icons: HashMap<u32, String>,
    pub status: String,
    pub live: live::Live,
    recorder: Option<live::Recorder>,
    pub fill: String,
    dir: Option<PathBuf>,
    /// settings.json as read, so that saving one setting keeps the others ("repo")
    settings: Value,
    /// "Save live captures": each pull's live events are saved next to its log for tools/livetest.py
    pub capture: bool,
    /// the last capture: when, and where it went or what went wrong
    pub capture_note: Option<(Instant, String)>,
    /// the checkout from settings.json; None asks the running checker
    repo: Option<PathBuf>,
    /// the running pull as judged live; shown while following the newest pull
    live_raw: Option<Value>,
    pub live_report: Option<Report>,
    /// the running Scrapper pull as judged live (`/api/scrapper/panel` of it)
    pub live_scrap: Option<Value>,
    /// the pull whose log has come in: no more live judging until the next pull starts
    closed_pull: u64,
    pub history: Vec<Hist>,
    /// the pull picked at the top; None follows the newest
    pub selected: Option<String>,
    pub latest: String,
    pub list_open: bool,
    /// the build the pull list and the arrows keep to: "" every build, "berserker" or "scrapper"
    pub build: &'static str,
    pub lstat: LiveStatus,
    /// what the shown pull is: "berserker" (the Splits report) or "scrapper" (`scrap` below)
    pub kind: String,
    /// the scrapper pull as the checker reduced it (`/api/scrapper/panel`)
    pub scrap: Option<Value>,
    /// the bench rotation for the scrapper cheat sheet (`/api/scrapper/sheet`)
    pub sheet: Option<Value>,
    pub show_sheet: bool,
    pub scrap_all: bool,
    /// your elite spec from the arcdps feed (18 Berserker, 43 Scrapper; 0 not seen yet)
    pub self_elite: u32,
}

/// How the live capture is doing, for the status at the top of the panel.
#[derive(Default)]
pub struct LiveStatus {
    /// your last event on the arcdps feed
    feed_at: Option<Instant>,
    feed_events: u64,
    posted_at: Option<Instant>,
    ok_at: Option<Instant>,
    err: Option<(Instant, String)>,
    posts: u32,
    oks: u32,
    fails: u32,
    starts: u32,
    started_at: Option<Instant>,
}

/// One status: a short line, its tone (0 muted, 1 ok, 2 waiting, 3 bad) and the details.
pub struct LiveLine {
    pub text: String,
    pub tone: u8,
    pub details: Vec<String>,
}

fn ago(t: Option<Instant>) -> String {
    match t {
        None => "never".into(),
        Some(t) => {
            let s = t.elapsed().as_secs_f32();
            if s < 60.0 {
                format!("{s:.1} s ago")
            } else {
                format!("{}:{:02} ago", s as u32 / 60, s as u32 % 60)
            }
        }
    }
}

impl State {
    pub fn live_line(&self) -> LiveLine {
        let st = &self.lstat;
        let in_pull = self.live.in_pull();
        let closed = self.live.first_hit() == self.closed_pull && self.closed_pull != 0;
        let err_new = st.err.as_ref().filter(|(t, _)| st.ok_at.map_or(true, |o| *t > o));
        let (text, tone) = if !SHOW_LIVE.load(Ordering::Relaxed) {
            ("Live off".to_string(), 0)
        } else if st.feed_at.is_none() {
            ("Live: no arcdps feed yet".to_string(), 0)
        } else if let Some((_, e)) = err_new {
            (format!("Live: {}", e.chars().take(48).collect::<String>()), 3)
        } else if in_pull && closed {
            ("Live: pull over, log in".to_string(), 0)
        } else if in_pull && st.ok_at.map_or(false, |t| t.elapsed().as_secs() < 4) {
            (format!("Live \u{2022} judged {}", ago(st.ok_at)), 1)
        } else if in_pull {
            ("Live: waiting for the judge".to_string(), 2)
        } else {
            (format!("Live ready \u{2022} last hit {}", ago(self.live.last_arrival)), 0)
        };
        let details = vec![
            format!("Your feed events: {}, last {}", st.feed_events, ago(st.feed_at)),
            format!("Last hit {}; this pull: {} hits{}", ago(self.live.last_arrival), self.live.hits, if closed { ", its log is in" } else { "" }),
            format!("Sent to the judge {} times, last {}", st.posts, ago(st.posted_at)),
            format!("Judged {} times, last {}; failed {}", st.oks, ago(st.ok_at), st.fails),
            match &st.err {
                Some((t, e)) => format!("Last error {}: {e}", ago(Some(*t))),
                None => "No errors".into(),
            },
            format!("Judge started {} times by the addon, last {}", st.starts, ago(st.started_at)),
            format!("Judge: {JUDGE} (core\\live_server.py); errors in core\\cache\\live_server.log"),
        ];
        LiveLine { text, tone, details }
    }
}

impl State {
    /// Picks a filler mode (one of `report::FILL_MODES`), judges the report again, remembers it.
    pub fn set_fill(&mut self, mode: &str) {
        self.fill = mode.to_string();
        if let Some(v) = &self.raw {
            self.report = report::digest(v, &self.fill);
        }
        if let Some(v) = &self.live_raw {
            self.live_report = report::digest(v, &self.fill);
        }
        self.save_setting("fill", self.fill.clone().into());
    }

    /// Sets one key of settings.json and writes it, keeping the others.
    fn save_setting(&mut self, key: &str, v: Value) {
        if !self.settings.is_object() {
            self.settings = serde_json::json!({});
        }
        self.settings[key] = v;
        if let Some(d) = &self.dir {
            let _ = std::fs::write(d.join("settings.json"), self.settings.to_string());
        }
    }

    /// Turns "Save live captures" on or off, and remembers it.
    pub fn set_capture(&mut self, on: bool) {
        self.capture = on;
        self.save_setting("capture", on.into());
    }

    /// The running pull's capture for `core/live_server.py` `/capture`, when it has one: the events
    /// last sent to the judge and what was drawn of them.
    fn capture_body(&self, log: &str) -> Option<String> {
        if !self.capture || self.live.hits == 0 || self.live.first_hit() == self.closed_pull {
            return None;
        }
        let (kind, shown) = match (&self.live_raw, &self.live_scrap) {
            (Some(d), _) => ("berserker", d.clone()),
            (None, Some(d)) => ("scrapper", d.clone()),
            _ => ("", Value::Null),
        };
        let mut body = self.live.to_json(self.self_elite);
        body["log"] = log.into();
        body["shown"] = serde_json::json!({"kind": kind, "data": shown});
        Some(body.to_string())
    }
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static SHOW: AtomicBool = AtomicBool::new(true);
pub static SHOW_LIVE: AtomicBool = AtomicBool::new(true);
static STOP: AtomicBool = AtomicBool::new(false);
static POLLER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
static JUDGER: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);
/// A pick from the panel for the poller: Some(None) = follow the newest, Some(Some(log)).
pub static WANT: Mutex<Option<Option<String>>> = Mutex::new(None);

/// Set by the first panic; from then on the addon draws only a note and ignores events.
static FAILED: Mutex<Option<String>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.lock().ok()?.as_mut().map(f)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Runs a callback from the game. A panic must not unwind into Nexus (that takes the whole game
/// down), so it is caught here, logged by the panic hook to the Nexus log, and the addon stops.
fn guarded(f: impl FnOnce()) {
    if lock(&FAILED).is_some() {
        return;
    }
    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".into());
        *lock(&FAILED) = Some(msg);
    }
}

fn load() {
    *lock(&FAILED) = None;
    // nexus's own hook also pops up a message box, which blocks the game: log only
    std::panic::set_hook(Box::new(|info| {
        let trace = std::backtrace::Backtrace::force_capture();
        nexus::log::log(nexus::log::LogLevel::Critical, "Bench Buddy", format!("{info}\n{trace:#}"));
    }));
    let dir = get_addon_dir("bench_buddy").ok();
    let recorder = dir.as_ref().and_then(|d| live::Recorder::start(d).ok());
    let settings = dir
        .as_ref()
        .and_then(|d| std::fs::read_to_string(d.join("settings.json")).ok())
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or(Value::Null);
    let fill = settings["fill"]
        .as_str()
        .map(String::from)
        .filter(|m| report::FILL_MODES.iter().any(|(k, _)| k == m))
        .unwrap_or_else(|| "budget".into());
    // where core/live_server.py is: settings.json may name a checkout, else the running checker says
    let repo = settings["repo"].as_str().map(PathBuf::from);
    let capture = settings["capture"].as_bool().unwrap_or(false);
    *lock(&STATE) = Some(State {
        raw: None,
        report: None,
        icons: HashMap::new(),
        status: "waiting for the checker (run.bat)".into(),
        live: live::Live::default(),
        recorder,
        fill,
        dir,
        settings,
        capture,
        capture_note: None,
        repo,
        live_raw: None,
        live_report: None,
        live_scrap: None,
        closed_pull: 0,
        history: Vec::new(),
        selected: None,
        latest: String::new(),
        list_open: false,
        build: "",
        lstat: LiveStatus::default(),
        kind: "berserker".into(),
        scrap: None,
        sheet: None,
        show_sheet: false,
        scrap_all: false,
        self_elite: 0,
    });
    STOP.store(false, Ordering::SeqCst);
    *lock(&POLLER) = Some(std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(poll);
    }));
    *lock(&JUDGER) = Some(std::thread::spawn(|| {
        let _ = std::panic::catch_unwind(judge);
    }));

    panel::load_fonts();
    register_render(RenderType::Render, render!(|ui| { guarded(|| frame(ui)); failed_note(ui) })).revert_on_unload();
    register_render(RenderType::OptionsRender, render!(|ui| guarded(|| options(ui)))).revert_on_unload();
    register_keybind_with_string(
        "BENCH_BUDDY_TOGGLE",
        keybind_handler!(|_, release| if !release {
            SHOW.fetch_xor(true, Ordering::Relaxed);
        }),
        "CTRL+SHIFT+F8",
    )
    .revert_on_unload();
    COMBAT_SQUAD.subscribe(event_consume!(<CombatData> |d| guarded(|| on_combat("area", d)))).revert_on_unload();
    COMBAT_LOCAL.subscribe(event_consume!(<CombatData> |d| guarded(|| on_combat("local", d)))).revert_on_unload();
}

fn unload() {
    STOP.store(true, Ordering::SeqCst);
    for t in [&POLLER, &JUDGER] {
        if let Some(h) = lock(t).take() {
            let _ = h.join();
        }
    }
    with(|s| {
        if let Some(r) = s.recorder.as_mut() {
            r.flush();
        }
    });
    *lock(&STATE) = None;
}

fn failed_note(ui: &Ui) {
    let Some(msg) = lock(&FAILED).clone() else { return };
    Window::new("Golem Splits")
        .size([420.0, 110.0], Condition::FirstUseEver)
        .build(ui, || {
            ui.text_colored(panel::BAD, "Stopped after an error; the game is fine.");
            ui.text_wrapped(&msg);
            ui.text_colored(panel::MUTED, "Details in addons\\Nexus\\Nexus.log. Unload and load it in Nexus to restart.");
        });
}

//// Asks the checker every 2 s for the newest log, and fetches the report of the pull to show
/// (the picked one, else the newest) when that changes.
fn poll() {
    let mut shown = String::new();
    let mut latest = String::new();
    let mut tick = 0u32;
    while !STOP.load(Ordering::SeqCst) {
        let want = lock(&WANT).take();
        if let Some(w) = &want {
            with(|s| s.selected = w.clone());
        }
        if tick % 20 == 0 || want.is_some() {
            if let Err(e) = poll_once(&mut shown, &mut latest) {
                with(|s| s.status = format!("checker not reachable on {CHECKER} ({e})"));
            }
        }
        tick += 1;
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn get_json(path: &str) -> Result<Value, String> {
    serde_json::from_str(&report::get(CHECKER, path)?).map_err(|e| e.to_string())
}

fn hist_of(h: &Value, kind: &'static str, count: &str) -> Vec<Hist> {
    h.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    Some(Hist {
                        log: x["log"].as_str()?.to_string(),
                        kind,
                        secs: x["secs"].as_f64().unwrap_or(0.0),
                        loops: x[count].as_i64().unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Berserker and Scrapper golem logs side by side: the newest of either is shown, and the kind of
/// the shown log picks the view.
fn poll_once(shown: &mut String, latest: &mut String) -> Result<(), String> {
    let v = get_json("/api/latest")?;
    let sv = get_json("/api/scrapper/latest").unwrap_or(Value::Null);
    let (bl, sl) = (v["latest"].as_str().unwrap_or(""), sv["latest"].as_str().unwrap_or(""));
    if bl.is_empty() && sl.is_empty() {
        return Ok(());
    }
    let stem = if sl > bl { sl } else { bl };
    let mut capture = None;
    if stem != latest {
        let mut hist = hist_of(&get_json("/api/history")?, "berserker", "loops");
        if !sl.is_empty() {
            hist.extend(hist_of(&get_json("/api/scrapper/history")?, "scrapper", "cycles"));
        }
        hist.sort_by(|a, b| a.log.cmp(&b.log));
        let first = latest.is_empty();
        *latest = stem.to_string();
        if !first {
            capture = with(|s| s.capture_body(stem)).flatten();
        }
        with(|s| {
            s.history = hist;
            s.latest = stem.to_string();
            // the running pull's log is in: it replaces the live view
            if !first {
                s.closed_pull = s.live.first_hit();
                s.live.end_pull();
                s.live_report = None;
                s.live_raw = None;
                s.live_scrap = None;
            }
        });
    }
    if let Some(body) = capture {
        let note = match report::post(JUDGE, "/capture", &body) {
            Ok(t) => serde_json::from_str::<Value>(&t).ok().and_then(|v| v["saved"].as_str().map(|p| format!("saved {p}")))
                .unwrap_or_else(|| format!("not saved: {t}")),
            Err(e) => format!("not saved: {e}"),
        };
        with(|s| s.capture_note = Some((Instant::now(), note)));
    }
    let target = with(|s| s.selected.clone()).flatten().unwrap_or_else(|| latest.clone());
    if target == *shown {
        return Ok(());
    }
    let scrapper = with(|s| s.history.iter().any(|h| h.log == target && h.kind == "scrapper")).unwrap_or(false);
    if scrapper {
        let icons = if with(|s| !s.icons.contains_key(&30088)).unwrap_or(true) {
            get_json("/api/scrapper/icons")?.as_object().map(|m| {
                m.iter()
                    .filter_map(|(k, u)| Some((k.parse().ok()?, u.as_str()?.to_string())))
                    .collect::<HashMap<u32, String>>()
            })
        } else {
            None
        };
        let sheet = if with(|s| s.sheet.is_none()).unwrap_or(true) { get_json("/api/scrapper/sheet").ok() } else { None };
        let raw = get_json(&format!("/api/scrapper/panel?log={target}"))?;
        *shown = target;
        with(|s| {
            s.kind = "scrapper".into();
            s.scrap = Some(raw);
            if let Some(i) = icons {
                s.icons.extend(i);
            }
            if sheet.is_some() {
                s.sheet = sheet;
            }
            s.status.clear();
        });
        return Ok(());
    }
    let icons = if with(|s| !s.icons.contains_key(&29923)).unwrap_or(true) {
        get_json("/api/icons")?.as_object().map(|m| {
            m.iter()
                .filter_map(|(k, u)| Some((k.parse().ok()?, u.as_str()?.to_string())))
                .collect::<HashMap<u32, String>>()
        })
    } else {
        None
    };
    let raw = get_json(&format!("/api/compare?log={target}"))?;
    *shown = target;
    with(|s| {
        s.kind = "berserker".into();
        s.report = report::digest(&raw, &s.fill);
        s.raw = Some(raw);
        if let Some(i) = icons {
            s.icons.extend(i);
        }
        s.status.clear();
    });
    Ok(())
}

/// While a pull runs, sends its casts to the live judge once a second when something is new.
fn judge() {
    let mut sent = u64::MAX;
    let mut spawned: Option<std::time::Instant> = None;
    while !STOP.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(250));
        let job = with(|s| {
            let fresh = s.live.in_pull() && s.live.first_hit() != s.closed_pull && s.live.hits > 0;
            (fresh && s.live.version != sent).then(|| (s.live.to_json(s.self_elite).to_string(), s.live.version, s.repo.clone()))
        })
        .flatten();
        let Some((body, version, repo)) = job else { continue };
        with(|s| {
            s.lstat.posted_at = Some(Instant::now());
            s.lstat.posts += 1;
        });
        match report::post(JUDGE, "/compare", &body) {
            Ok(text) => {
                sent = version;
                with(|s| {
                    s.lstat.ok_at = Some(Instant::now());
                    s.lstat.oks += 1;
                });
                // {"kind": the build, "data": what the checker gives for a log of it}
                if let Ok(mut raw) = serde_json::from_str::<Value>(&text) {
                    with(|s| {
                        let data = raw["data"].take();
                        if s.live.first_hit() == s.closed_pull || !data.is_object() {
                            return;
                        }
                        match raw["kind"].as_str() {
                            Some("berserker") => {
                                s.live_report = report::digest(&data, &s.fill);
                                s.live_raw = Some(data);
                            }
                            Some("scrapper") => s.live_scrap = Some(data),
                            _ => {}
                        }
                    });
                }
            }
            Err(e) if e == report::REFUSED => {
                let start = spawned.map_or(true, |t| t.elapsed().as_secs() > 20);
                with(|s| {
                    s.lstat.fails += 1;
                    s.lstat.err = Some((Instant::now(), "judge not running, starting it".into()));
                    if start {
                        s.lstat.starts += 1;
                        s.lstat.started_at = Some(Instant::now());
                    }
                });
                if start {
                    spawned = Some(Instant::now());
                    start_judge(repo.as_deref());
                }
            }
            Err(e) => {
                with(|s| {
                    s.lstat.fails += 1;
                    s.lstat.err = Some((Instant::now(), e));
                });
            }
        }
        std::thread::sleep(Duration::from_millis(750));
    }
}

/// Starts `core/live_server.py` with pythonw, without a window, from the checkout named in
/// settings.json or else the one the running checker runs from (`/api/root`).
fn start_judge(repo: Option<&std::path::Path>) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    let repo = match repo {
        Some(r) => r.to_path_buf(),
        None => match get_json("/api/root").ok().and_then(|v| v["root"].as_str().map(PathBuf::from)) {
            Some(r) => r,
            None => {
                with(|s| s.lstat.err = Some((Instant::now(), "checker not running: start run.bat".into())));
                return;
            }
        },
    };
    let script = repo.join("core").join("live_server.py");
    if !script.exists() {
        with(|s| s.lstat.err = Some((Instant::now(), format!("judge not found: {}", script.display()))));
        return;
    }
    let r = std::process::Command::new("pythonw")
        .arg(&script)
        .current_dir(&repo)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();
    if let Err(e) = r {
        with(|s| s.lstat.err = Some((Instant::now(), format!("could not start the judge: {e}"))));
    }
}

fn on_combat(feed: &'static str, data: Option<&CombatData>) {
    let Some(d) = data else { return };
    let (Some(e), Some(src)) = (d.event(), d.src()) else { return };
    if src.is_self == 0 {
        return;
    }
    with(|s| {
        if src.elite != 0 && src.elite != u32::MAX {
            s.self_elite = src.elite;
        }
        s.lstat.feed_at = Some(Instant::now());
        s.lstat.feed_events += 1;
        let t = s.recorder.as_ref().map_or(0, |r| r.now_us());
        if let Some(r) = s.recorder.as_mut() {
            r.write(feed, e, t);
        }
        s.live.event(feed, e, d.dst().is_some_and(|a| a.is_self != 0));
    });
}

fn options(ui: &Ui) {
    let mut show = SHOW.load(Ordering::Relaxed);
    if ui.checkbox("Show the Splits panel (CTRL+SHIFT+F8)", &mut show) {
        SHOW.store(show, Ordering::Relaxed);
    }
    let mut lv = SHOW_LIVE.load(Ordering::Relaxed);
    if ui.checkbox("Live row while a pull runs", &mut lv) {
        SHOW_LIVE.store(lv, Ordering::Relaxed);
    }
    with(|s| {
        let mut cap = s.capture;
        if ui.checkbox("Save live captures next to the arcdps logs", &mut cap) {
            s.set_capture(cap);
        }
        ui.text_disabled("What the live judge saw of each pull, as <log>.live.json, for tools/livetest.py. Stays on this PC.");
        if let Some((t, n)) = &s.capture_note {
            ui.text_disabled(format!("Last capture {}: {n}", ago(Some(*t))));
        }
    });
    ui.text_disabled(format!("Reads the checker on {CHECKER}; start it with run.bat."));
}

fn frame(ui: &Ui) {
    if !SHOW.load(Ordering::Relaxed) {
        return;
    }
    let mut guard = lock(&STATE);
    let Some(s) = guard.as_mut() else { return };
    if !panel::draw(ui, s) {
        SHOW.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_panic_is_caught_and_stops_the_addon() {
        super::guarded(|| panic!("boom"));
        assert_eq!(super::lock(&super::FAILED).as_deref(), Some("boom"));
        let mut ran = false;
        super::guarded(|| ran = true);
        assert!(!ran);
    }
}

#[cfg(test)]
mod judge_tests {
    /// `BB_SPAWN=1 cargo test` starts the live judge as the addon does and asks it.
    #[test]
    fn starts_the_live_judge() {
        if std::env::var("BB_SPAWN").is_err() {
            return;
        }
        super::start_judge(None);
        std::thread::sleep(std::time::Duration::from_secs(4));
        let r = crate::report::get(super::JUDGE, "/");
        assert!(r.is_ok(), "{r:?}");
    }
}

#[cfg(test)]
mod replay_tests {
    /// `BB_REPLAY=<combat.jsonl> BB_ELITE=43 cargo test --release replay -- --nocapture` sends a
    /// recorded feed's events to the running live judge as the addon does at the end of each pull
    /// (a 12 s gap in your hits) and prints what came back. tools/livereplay.py does the same
    /// against the logs.
    #[test]
    fn replay() {
        let Ok(path) = std::env::var("BB_REPLAY") else { return };
        let elite: u32 = std::env::var("BB_ELITE").ok().and_then(|e| e.parse().ok()).unwrap_or(18);
        let lines: Vec<serde_json::Value> = std::fs::read_to_string(path).unwrap().lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        // your agent: the target of your initial buffs (older recordings don't say)
        let mut n = std::collections::HashMap::new();
        for j in &lines {
            if j["ev"]["is_statechange"] == 18 {
                *n.entry(j["ev"]["dst_agent"].as_u64().unwrap()).or_insert(0) += 1;
            }
        }
        let me = n.into_iter().max_by_key(|x| x.1).map(|x| x.0);
        let judge = |events: &[serde_json::Value]| {
            let body = serde_json::json!({"elite": elite, "events": events}).to_string();
            let text = crate::report::post(super::JUDGE, "/compare", &body).expect("judge");
            let raw: serde_json::Value = serde_json::from_str(&text).unwrap();
            let rows = raw["data"]["rows"].as_array().map_or(0, |r| r.len());
            println!("pull: {} events, judged as {}, {rows} rows", events.len(), raw["kind"]);
        };
        let (mut pull, mut last) = (Vec::new(), 0u64);
        for j in lines.iter().filter(|j| j["feed"] == "area") {
            let e = &j["ev"];
            let (t, sc) = (e["time"].as_u64().unwrap(), e["is_statechange"].as_u64().unwrap());
            let direct = sc == 0 && e["buff"] == 0 && e["dst_agent"] != 0;
            if direct && last != 0 && t > last + 12_000 {
                judge(&pull);
                pull.clear();
            }
            if direct {
                last = t;
            }
            if matches!(sc, 0 | 18 | 69 | 72) {
                pull.push(serde_json::json!({"time": t, "sc": sc, "buff": e["buff"], "skill": e["skill_id"], "value": e["value"],
                    "buff_dmg": e["buff_dmg"], "result": e["result"], "buffremove": e["is_buffremove"], "dst": e["dst_agent"],
                    "dst_self": Some(e["dst_agent"].as_u64().unwrap()) == me}));
            }
        }
        if !pull.is_empty() {
            judge(&pull);
        }
    }
}
