//! The Bench Buddy checker: http://127.0.0.1:8771/ opens the build of the newest golem log;
//! /berserker (Condi Berserker) and /scrapper (Power Scrapper).
//!
//!     run.bat                                   (cargo run --release -p checker, restarted on changes)
//!     checker [port] [--no-browser]             the pages
//!     checker api /api/scrapper/compare?log=<stem>   one route's JSON, without the server
//!     checker live <capture.live.json>          a saved live capture judged as the addon judges it
//!     checker snapshot [--save]                 every answer against a saved run (see snapshot.rs)
//!
//! Watches the golem logs: each page asks every few seconds for the newest log of its build and shows
//! it as soon as arcdps has written it. Nothing is uploaded; each build's bench log is downloaded once
//! from dps.report into modules/<build>/cache/. The pages are read from modules/ on every request and
//! the open page reloads itself when one changes. A change to the Rust code or the live cast table
//! ends the process with exit code 3, which run.bat answers by building and starting it again.
//!
//! For the tests, POST /api/live judges {"elite", "events"} as the addon does, and POST /api/capture
//! saves a live capture.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

mod snapshot;

use bb_app::{App, Caches};
use serde_json::{json, Value};

const PORT: u16 = 8771;
/// exit code that run.bat answers by building and starting the checker again
const RESTART: i32 = 3;
const VIEWS: [&str; 4] = ["splits", "grid", "timeline", "coach"];

/// A build picker in the top right corner of every page, filled from /api/modules.
const BUILDS_HTML: &str = r##"<div id="bb-builds" style="position:fixed;top:8px;right:12px;z-index:100;font:12px system-ui,sans-serif">
<select style="background:#1c1c1b;color:#c3c2b7;border:1px solid #3a3a38;border-radius:6px;padding:3px 6px;font:inherit"></select></div>
<script>(async () => {
  const sel = document.querySelector("#bb-builds select");
  const here = location.pathname.split("/")[1];
  const mods = await fetch("/api/modules").then(r => r.json()).catch(() => []);
  for (const m of mods) {
    const o = new Option(m.name, m.route, false, m.route === here);
    sel.add(o);
  }
  sel.onchange = () => { location.href = "/" + sel.value; };
})();</script>
"##;

/// Hides each mockup's fake game backdrop: here the page is a plain window next to the game.
const EMBED_CSS: &str = r#"<style>
.game, .stage > .floor, .stage > .skillbar, .stage > .char, .stage > .golem,
#screen > .ground, #screen > .golem, #screen > .char, #screen > .bar, #stage > .fake, #stage > .fake2 { display: none !important; }
html, body, .stage, #screen, #stage { background: #0d0d0d !important; }
</style>"#;

struct Server {
    app: App,
    root: PathBuf,
    started: u64,
    version: AtomicU64,
}

/// The repository: the folder with modules/ above the running program or the current folder.
fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let exe = std::env::current_exe().ok();
    [Some(cwd.clone()), exe].into_iter().flatten()
        .flat_map(|p| p.ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
        .find(|p| p.join("modules").is_dir() && p.join("crates").is_dir())
        .unwrap_or(cwd)
}

fn query(q: &str) -> HashMap<String, String> {
    q.split('&').filter(|x| !x.is_empty()).map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (unescape(k), unescape(v))
    }).collect()
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()).ok_or(()) {
                    Ok(x) => {
                        out.push(x);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

enum Reply {
    Json(Value),
    Html(String),
    Go(String),
    Missing,
    Fail(String),
}

fn with_builds(html: &str) -> String {
    match html.rfind("</body>") {
        Some(i) => format!("{}{BUILDS_HTML}{}", &html[..i], &html[i..]),
        None => format!("{html}{BUILDS_HTML}"),
    }
}

impl Server {
    fn page(&self, rel: &[&str]) -> Reply {
        let mut p = self.root.join("modules");
        for r in rel {
            p = p.join(r);
        }
        match std::fs::read_to_string(&p) {
            Ok(s) => Reply::Html(with_builds(&s)),
            Err(e) => Reply::Fail(format!("{}: {e}", p.display())),
        }
    }

    /// One of the Berserker views with this pull's data inlined.
    fn view(&self, name: &str, stem: &str) -> Result<Reply, String> {
        let p = self.root.join("modules").join("berserker").join("views").join(format!("{name}.html"));
        let html = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
        let icons = self.app.api("/api/icons", &HashMap::new())?.unwrap_or(Value::Null);
        let data = format!("window.PULL = {};\nwindow.ICONS = {};", self.app.bers_compare(stem)?, icons);
        let html = html.replacen(r#"<script src="data.js"></script>"#, &format!("<script>{}</script>", data.replace("</", "<\\/")), 1);
        Ok(Reply::Html(html.replacen("</head>", &format!("{EMBED_CSS}</head>"), 1)))
    }

    fn get(&self, path: &str, q: &HashMap<String, String>, raw_q: &str) -> Result<Reply, String> {
        if path != "/" && path.ends_with('/') {
            let to = path.trim_end_matches('/');
            return Ok(Reply::Go(if raw_q.is_empty() { to.to_string() } else { format!("{to}?{raw_q}") }));
        }
        Ok(match path {
            "/" => Reply::Go(format!("/{}", self.app.newest_route())),
            "/berserker" => self.page(&["berserker", "page.html"]),
            "/scrapper" => self.page(&["scrapper", "grid.html"]),
            "/api/version" => Reply::Json(json!({"v": format!("{}-{}", self.started, self.version.load(Ordering::Relaxed))})),
            "/api/root" => Reply::Json(json!({"root": self.root.display().to_string()})),
            _ => {
                if let Some(name) = path.strip_prefix("/view/") {
                    if VIEWS.contains(&name) {
                        return self.view(name, q.get("log").ok_or("no log= given")?);
                    }
                }
                if let Some(name) = path.strip_prefix("/scrapper/") {
                    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())
                        && self.root.join("modules").join("scrapper").join(format!("{name}.html")).exists() {
                        return Ok(self.page(&["scrapper", &format!("{name}.html")]));
                    }
                }
                if let Some(m) = self.app.simple.iter().find(|m| path == format!("/{}", m.route())) {
                    return Ok(self.page(&[m.route(), m.page()]));
                }
                match self.app.api(path, q)? {
                    Some(v) => Reply::Json(v),
                    None => Reply::Missing,
                }
            }
        })
    }

    fn post(&self, path: &str, body: &str) -> Result<Reply, String> {
        let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
        Ok(match path {
            "/api/live" => Reply::Json(self.app.live(&v)?),
            "/api/capture" => Reply::Json(self.app.save_capture(&v)?),
            _ => Reply::Missing,
        })
    }

    fn handle(&self, mut s: TcpStream) {
        let _ = s.set_read_timeout(Some(Duration::from_secs(30)));
        let mut r = BufReader::new(match s.try_clone() { Ok(c) => c, Err(_) => return });
        let mut line = String::new();
        if r.read_line(&mut line).is_err() {
            return;
        }
        let mut parts = line.split_whitespace();
        let (method, target) = (parts.next().unwrap_or(""), parts.next().unwrap_or("/"));
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if r.read_line(&mut h).is_err() || h.trim().is_empty() {
                break;
            }
            if let Some((k, v)) = h.split_once(':') {
                if k.trim().eq_ignore_ascii_case("content-length") {
                    len = v.trim().parse().unwrap_or(0);
                }
            }
        }
        let (path, raw_q) = target.split_once('?').unwrap_or((target, ""));
        let path = unescape(path);
        let reply = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if method == "POST" {
                let mut b = vec![0; len];
                r.read_exact(&mut b).map_err(|e| e.to_string())?;
                self.post(&path, &String::from_utf8_lossy(&b))
            } else {
                self.get(&path, &query(raw_q), raw_q)
            }
        })).unwrap_or_else(|e| Err(e.downcast_ref::<String>().cloned().or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_else(|| "panic".into()))).unwrap_or_else(Reply::Fail);
        let (code, ctype, body, extra) = match reply {
            Reply::Json(v) => ("200 OK", "application/json", v.to_string(), String::new()),
            Reply::Html(h) => ("200 OK", "text/html", h, String::new()),
            Reply::Go(to) => ("302 Found", "text/plain", String::new(), format!("Location: {to}\r\n")),
            Reply::Missing => ("404 Not Found", "text/plain", "not found".into(), String::new()),
            Reply::Fail(e) => ("500 Internal Server Error", "text/plain", e, String::new()),
        };
        let head = format!("HTTP/1.1 {code}\r\nContent-Type: {ctype}; charset=utf-8\r\nCache-Control: no-store\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n", body.len());
        let _ = s.write_all(head.as_bytes()).and_then(|_| s.write_all(body.as_bytes()));
    }
}

/// The newest change time of the files under `dir` whose names end in one of `ends`.
fn newest(dir: &Path, ends: &[&str], skip: &[&str]) -> SystemTime {
    let mut t = SystemTime::UNIX_EPOCH;
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if skip.contains(&name.as_str()) {
            continue;
        }
        if p.is_dir() {
            t = t.max(newest(&p, ends, skip));
        } else if ends.iter().any(|x| name.ends_with(x)) {
            t = t.max(e.metadata().and_then(|m| m.modified()).unwrap_or(t));
        }
    }
    t
}

/// Every second: a changed page bumps the version the open page watches; changed code ends the
/// process with RESTART.
fn watch(srv: Arc<Server>) {
    let code = |r: &Path| newest(&r.join("crates"), &[".rs", ".toml", ".json"], &["target", "cache"])
        .max(newest(&r.join("core"), &[".json"], &["cache"])).max(newest(r, &["Cargo.toml"], &["target", "crates", "modules", "addon", "core", "tools"]));
    let pages = |r: &Path| newest(&r.join("modules"), &[".html"], &["cache"]);
    let (c0, mut p0) = (code(&srv.root), pages(&srv.root));
    loop {
        std::thread::sleep(Duration::from_secs(1));
        if code(&srv.root) != c0 {
            println!("the code changed: building and starting again");
            std::process::exit(RESTART);
        }
        let p = pages(&srv.root);
        if p != p0 {
            p0 = p;
            srv.version.fetch_add(1, Ordering::Relaxed);
            println!("a page changed: the open page reloads");
        }
    }
}

fn caches(root: &Path) -> Caches {
    match std::env::var_os("BENCH_BUDDY_CACHE") {
        Some(d) => Caches::Flat(PathBuf::from(d)),
        None => Caches::Repo(root.to_path_buf()),
    }
}

fn fail(e: &str) -> ! {
    eprintln!("checker: {e}");
    std::process::exit(1)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    match args.first().map(String::as_str) {
        Some("api") => {
            let app = App::new(caches(&root));
            let t = args.get(1).unwrap_or_else(|| fail("usage: checker api <path?query>"));
            let (p, q) = t.split_once('?').unwrap_or((t, ""));
            match app.api(p, &query(q)) {
                Ok(Some(v)) => println!("{v}"),
                Ok(None) => fail("no such route"),
                Err(e) => fail(&e),
            }
            return;
        }
        Some("snapshot") => {
            let app = App::new(caches(&root));
            std::process::exit(snapshot::run(&app, &root.join("cache").join("snapshot"), args.iter().any(|a| a == "--save")));
        }
        Some("live") => {
            let app = App::new(caches(&root));
            let f = args.get(1).unwrap_or_else(|| fail("usage: checker live <capture.live.json>"));
            let body: Value = serde_json::from_str(&std::fs::read_to_string(f).unwrap_or_else(|e| fail(&e.to_string())))
                .unwrap_or_else(|e| fail(&e.to_string()));
            println!("{}", app.live(&body).unwrap_or_else(|e| fail(&e)));
            return;
        }
        _ => {}
    }
    let port = args.iter().find_map(|a| a.parse::<u16>().ok()).unwrap_or(PORT);
    let srv = Arc::new(Server {
        app: App::new(caches(&root)),
        root: root.clone(),
        started: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        version: AtomicU64::new(0),
    });
    let listener = TcpListener::bind(("127.0.0.1", port)).unwrap_or_else(|e| fail(&format!("port {port}: {e}")));
    let url = format!("http://127.0.0.1:{port}/");
    println!("Bench Buddy on {url}: Berserker on {url}berserker, Scrapper on {url}scrapper (Ctrl+C to stop; code changes are picked up by themselves)");
    for m in &srv.app.simple {
        println!("  {} on {url}{}", m.name(), m.route());
    }
    if !args.iter().any(|a| a == "--no-browser") {
        let _ = std::process::Command::new("cmd").args(["/c", "start", "", &url]).spawn();
    }
    let w = srv.clone();
    std::thread::spawn(move || watch(w));
    for s in listener.incoming().flatten() {
        let srv = srv.clone();
        std::thread::spawn(move || srv.handle(s));
    }
}
