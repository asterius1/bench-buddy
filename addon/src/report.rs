//! The Splits report: fetched from the running checker (`core/serve.py`, `/api/compare`) and
//! reduced to what the panel draws. The reduction follows `modules/berserker/views/splits.html`, including
//! its four filler modes; when that page changes how it judges, change this with it.

use serde_json::Value;
use std::{
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};

/// Plain HTTP/1.0 GET against a local server; returns the body of a 200.
pub fn get(addr: &str, path: &str) -> Result<String, String> {
    request(addr, &format!("GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n"))
}

/// POST of a JSON body; returns the body of a 200.
pub fn post(addr: &str, path: &str, body: &str) -> Result<String, String> {
    request(
        addr,
        &format!(
            "POST {path} HTTP/1.0\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    )
}

/// The error text when nothing answers on the port.
pub const REFUSED: &str = "refused";

fn request(addr: &str, req: &str) -> Result<String, String> {
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("no address")?;
    // Windows answers a closed local port only after retrying for about 2 s, so a connect that
    // times out is taken as nothing listening, the same as a refusal
    let mut s = TcpStream::connect_timeout(&sock, Duration::from_millis(500)).map_err(|_| REFUSED.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(10))).ok();
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    let (head, body) = text.split_once("\r\n\r\n").ok_or("bad response")?;
    if !head.starts_with("HTTP/1.0 200") && !head.starts_with("HTTP/1.1 200") {
        // the status line, and the server's own error text when it sent one
        let first = head.lines().next().unwrap_or("");
        let why = serde_json::from_str::<Value>(body).ok().and_then(|v| v["error"].as_str().map(String::from));
        return Err(why.map_or(first.to_string(), |w| format!("{first}: {w}")));
    }
    Ok(body.to_string())
}

/// How the fillers are judged, as the page's buttons: by how late the closing SE starts after it
/// is ready again, by that lateness against the bench's, only the whole longbow visit, or by
/// section time.
pub const FILL_MODES: [(&str, &str); 4] =
    [("budget", "SE ready"), ("bench", "SE vs bench"), ("sandwich", "whole visit"), ("time", "section time")];

#[derive(Clone, Copy, PartialEq)]
pub enum Border {
    None,
    Err,
    Extra,
    Skip,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Tone {
    Ink,
    Muted,
    Bad,
}

#[derive(Clone)]
pub struct Chip {
    pub id: u32,
    pub name: String,
    pub kind: String,
    pub flags: Vec<String>,
    pub t: Option<f64>,
    pub dur: Option<f64>,
    pub ref_dur: Option<f64>,
    /// the bench cast it lines up with (for a missed one: the bench cast itself)
    pub ref_id: Option<u32>,
    pub ref_name: String,
    pub ref_t: Option<f64>,
    pub delta: Option<f64>,
    pub gap: Option<f64>,
    /// start to next cast, yours and the bench's, and the difference that is judged
    pub slot: Option<f64>,
    pub ref_slot: Option<f64>,
    pub lost: Option<f64>,
    pub cd_wait: Option<f64>,
    pub hits: Option<i64>,
    pub expect: Option<i64>,
    pub stop: String,
    /// ms behind the bench when this cast started
    pub lag: Option<f64>,
}

impl Chip {
    pub fn problem(&self) -> bool {
        self.kind == "ghost" || self.kind == "extra" || !self.flags.is_empty()
    }
    pub fn has(&self, f: &str) -> bool {
        self.flags.iter().any(|x| x == f)
    }
}

#[derive(Clone, Default)]
pub struct Window {
    pub budget: Option<f64>,
    pub used: Option<f64>,
    pub idle: Option<f64>,
    pub se_late: Option<f64>,
    pub fof_late: Option<f64>,
    pub closed: bool,
}

#[derive(Clone)]
pub struct Sec {
    pub key: String,
    pub label: String,
    pub long: String,
    /// right side of the section's top line, e.g. "+0.05" or "SE +0.12"
    pub text: String,
    pub tone: Tone,
    pub budget: String,
    pub tempo: Option<f64>,
    pub border: Border,
    pub chips: Vec<Chip>,
    pub time: Option<f64>,
    pub ref_time: Option<f64>,
}

#[derive(Clone)]
pub struct Part {
    pub kind: String,
    pub num: String,
    pub n: i64,
    pub ref_n: i64,
    pub name: &'static str,
    pub secs: Vec<Sec>,
    pub delta: f64,
    pub len: Option<f64>,
    pub ref_len: Option<f64>,
    pub lag_end: f64,
    pub bench_end: Option<f64>,
    /// the longbow visit's cooldown window, yours and the bench's
    pub cd: Option<(Window, Option<Window>)>,
}

#[derive(Clone, Default)]
pub struct Visits {
    pub n: i64,
    pub med: f64,
    pub min: f64,
    pub max: f64,
}

#[derive(Clone)]
pub struct Report {
    pub log: String,
    pub secs: f64,
    pub lag_end: f64,
    /// next cast late ("slow"), missed, missed hits, extra
    pub counts: [usize; 4],
    pub damage: Option<(usize, f64)>,
    pub parts: Vec<Part>,
    pub sword: Option<Visits>,
    pub bow: Option<Visits>,
}

/// A close SE within 0.15 s of being ready counts as on time (the bench is 0.2 to 0.44 s late).
pub const LATE_OK: f64 = 150.0;

fn f(v: &Value, k: &str) -> Option<f64> {
    v.get(k).and_then(Value::as_f64)
}

fn sec_names(k: &str) -> (&'static str, &'static str) {
    match k {
        "a" => ("start", "Sword start"),
        "b" => ("flurry", "Flurry part"),
        "open" => ("open", "Longbow open"),
        "fill" => ("fill", "Fillers"),
        "close" => ("close", "Close"),
        _ => ("", ""),
    }
}

/// "+0.12", "−0.05", "±0.00" (the page's minus sign)
pub fn sgn(ms: f64) -> String {
    let s = if ms > 0.0 { "+" } else if ms < 0.0 { "\u{2212}" } else { "±" };
    format!("{s}{:.2}", ms.abs() / 1000.0)
}

pub fn clock(ms: f64) -> String {
    let s = ms / 1000.0;
    format!("{}:{:04.1}", (s / 60.0).floor() as i64, s % 60.0)
}

fn window(v: Option<&Value>) -> Option<Window> {
    let v = v.filter(|v| v.is_object())?;
    Some(Window {
        budget: f(v, "budget"),
        used: f(v, "used"),
        idle: f(v, "idle"),
        se_late: f(v, "seLate"),
        fof_late: f(v, "fofLate"),
        closed: v["closed"].as_bool().unwrap_or(false),
    })
}

fn chip(c: &Value) -> Chip {
    let r = c.get("ref").filter(|r| r.is_object());
    let ghost = c["kind"] == "ghost";
    Chip {
        id: c.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
        name: c.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        kind: c.get("kind").and_then(Value::as_str).unwrap_or("ok").to_string(),
        flags: c
            .get("flags")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        t: f(c, "t"),
        dur: f(c, "dur"),
        ref_dur: if ghost { f(c, "refDur") } else { r.and_then(|r| f(r, "dur")).or(f(c, "refDur")) },
        ref_id: if ghost { c["id"].as_u64().map(|x| x as u32) } else { r.and_then(|r| r["id"].as_u64()).map(|x| x as u32) },
        ref_name: if ghost { c["name"].as_str() } else { r.and_then(|r| r["name"].as_str()) }.unwrap_or("").to_string(),
        ref_t: if ghost { f(c, "refT") } else { r.and_then(|r| f(r, "t")) },
        delta: f(c, "delta"),
        gap: f(c, "gap"),
        slot: f(c, "slot"),
        ref_slot: f(c, "refSlot"),
        lost: f(c, "lost"),
        cd_wait: f(c, "cdWait"),
        hits: c.get("hits").and_then(Value::as_i64),
        expect: c.get("expect").and_then(Value::as_i64),
        stop: c.get("stop").and_then(Value::as_str).unwrap_or("").to_string(),
        lag: None,
    }
}

fn visits(v: Option<&Value>) -> Option<Visits> {
    let v = v?;
    Some(Visits { n: v["n"].as_i64()?, med: f(v, "med")?, min: f(v, "min")?, max: f(v, "max")? })
}

pub fn digest(p: &Value, mode: &str) -> Option<Report> {
    let lag: Vec<Option<f64>> = p.get("lag")?.as_array()?.iter().map(Value::as_f64).collect();
    // parts in pull order
    let mut raw: Vec<(String, i64, i64, &Value)> = Vec::new();
    for r in p.get("rows")?.as_array()? {
        for k in ["opener", "sword", "bow"] {
            if let Some(x) = r.get(k) {
                let n = r.get("n").and_then(Value::as_i64).unwrap_or(0);
                let rn = r.get("refN").and_then(Value::as_i64).unwrap_or(n);
                raw.push((k.to_string(), n, rn, x));
            }
        }
    }
    let mut parts: Vec<Part> = Vec::new();
    let mut prev_end = 0.0;
    let mut bench_t = 0.0;
    let mut counts = [0usize; 4];
    let last_i = raw.len().saturating_sub(1);
    for (i, (kind, n, ref_n, x)) in raw.iter().enumerate() {
        let lag_start = if i == 0 { 0.0 } else { lag.get(i - 1).copied().flatten().unwrap_or(prev_end) };
        let secs_v = x.get("sections").and_then(Value::as_object);
        let all: Vec<Value> = secs_v
            .map(|m| m.values().flat_map(|s| s["chips"].as_array().cloned().unwrap_or_default()).collect())
            .unwrap_or_default();
        let has_ref = |c: &Value| c.get("ref").map_or(false, |r| r.is_object());
        let matched: Vec<&Value> = all.iter().filter(|c| has_ref(c) && f(c, "t").is_some()).collect();
        let reft = |c: &Value| c["ref"]["t"].as_f64().unwrap_or(0.0);
        let off = matched.first().map(|c| f(c, "t").unwrap() - reft(c) - lag_start).unwrap_or(0.0);
        let lag_at = |c: &Value| -> Option<f64> { if has_ref(c) { f(c, "t").map(|t| t - reft(c) - off) } else { None } };
        let len = f(x, "len");
        let ref_len = f(x, "refLen");
        let lag_end = if let Some(Some(l)) = lag.get(i) {
            *l
        } else if let (Some(l), Some(r)) = (len, ref_len) {
            lag_start + l - r
        } else {
            matched.last().and_then(|c| lag_at(c)).unwrap_or(lag_start)
        };
        prev_end = lag_end;
        let delta = lag_end - lag_start;
        let bench_end = match (len, ref_len) {
            (Some(_), Some(r)) => {
                bench_t += r;
                Some(bench_t)
            }
            _ => None,
        };
        let cd = if kind == "bow" { x.get("cd") } else { None };
        let cm = window(cd.and_then(|c| c.get("mine")));
        let cr = window(cd.and_then(|c| c.get("ref")));
        let mut secs = Vec::new();
        if let Some(m) = secs_v {
            for (sk, s) in m {
                let (label, long) = sec_names(sk);
                let time = f(s, "time");
                let ref_time = f(s, "refTime");
                let d = match (time, ref_time) {
                    (Some(a), Some(b)) => Some(a - b),
                    _ => None,
                };
                let mut border = Border::None;
                let mut tone = Tone::Ink;
                let (mut text, mut tempo) = if kind == "opener" {
                    (sgn(delta), Some(delta))
                } else if let Some(d) = d {
                    (sgn(d), Some(d))
                } else if i == last_i {
                    tone = Tone::Muted;
                    ("end".to_string(), None)
                } else {
                    border = Border::Skip;
                    tone = Tone::Bad;
                    ("skipped".to_string(), None)
                };
                let mut budget = String::new();
                if let Some(cm) = &cm {
                    if sk == "fill" && cm.closed {
                        budget = format!("{:.1} of {:.1} s", cm.used.unwrap_or(0.0) / 1000.0, cm.budget.unwrap_or(0.0) / 1000.0);
                        let rl = cr.as_ref().and_then(|r| r.se_late);
                        match (mode, cm.se_late) {
                            ("budget", Some(l)) => (text, tempo) = (format!("SE {}", sgn(l)), Some(l - LATE_OK)),
                            ("bench", Some(l)) if rl.is_some() => {
                                let d = l - rl.unwrap();
                                (text, tempo) = (format!("SE {}", sgn(d)), Some(d));
                            }
                            ("sandwich", _) => {
                                (text, tempo, tone) = ("window".into(), None, Tone::Muted);
                            }
                            _ => {}
                        }
                    }
                    if sk == "close" {
                        let rl = cr.as_ref().and_then(|r| r.fof_late);
                        match (mode, cm.fof_late) {
                            // the close starts whenever SE is ready and FoF may then wait for its
                            // own cooldown: judged by how late FoF came after being ready
                            ("budget", Some(l)) => (text, tempo) = (format!("FoF {}", sgn(l)), Some(l - LATE_OK)),
                            ("bench", Some(l)) if rl.is_some() => {
                                let d = l - rl.unwrap();
                                (text, tempo) = (format!("FoF {}", sgn(d)), Some(d));
                            }
                            ("sandwich", _) => (text, tempo, tone) = (sgn(delta), Some(delta), Tone::Ink),
                            _ => {}
                        }
                    }
                }
                let chips_v = s["chips"].as_array().cloned().unwrap_or_default();
                let mut chips = Vec::new();
                for c in chips_v.iter().filter(|c| c["kind"] != "cut") {
                    let mut ch = chip(c);
                    ch.lag = lag_at(c);
                    counts[0] += ch.has("slow") as usize;
                    counts[1] += (ch.kind == "ghost") as usize;
                    counts[2] += ch.has("few") as usize;
                    counts[3] += (ch.kind == "extra") as usize;
                    chips.push(ch);
                }
                if chips.iter().any(|c| c.kind == "ghost" || c.has("few") || c.has("nobuff")) {
                    border = Border::Err;
                } else if border == Border::None && chips.iter().any(|c| c.kind == "extra") {
                    border = Border::Extra;
                }
                secs.push(Sec {
                    key: sk.clone(),
                    label: label.into(),
                    long: long.into(),
                    text,
                    tone,
                    budget,
                    tempo,
                    border,
                    chips,
                    time,
                    ref_time,
                });
            }
        }
        parts.push(Part {
            kind: kind.clone(),
            num: if kind == "opener" { "0".into() } else { n.to_string() },
            n: *n,
            ref_n: *ref_n,
            name: match kind.as_str() {
                "opener" => "Opener",
                "sword" => "Sword",
                _ => "Bow",
            },
            secs,
            delta,
            len,
            ref_len,
            lag_end,
            bench_end,
            cd: cm.map(|m| (m, cr)),
        });
    }
    let damage = p.get("damage").and_then(|d| {
        let mine = d.get("mine")?.as_array()?;
        let rf = d.get("ref")?.as_array()?;
        let i = mine.len().checked_sub(1)?;
        let (a, b) = (mine[i].as_f64()?, rf.get(i)?.as_f64()?);
        (i > 0 && b > 0.0).then(|| (i, (a / b - 1.0) * 100.0))
    });
    Some(Report {
        log: p.get("log")?.as_str()?.to_string(),
        secs: f(p, "secs").unwrap_or(0.0),
        lag_end: parts.last().map_or(0.0, |x| x.lag_end),
        counts,
        damage,
        parts,
        sword: visits(p.get("sword")),
        bow: visits(p.get("bow")),
    })
}

#[cfg(test)]
mod tests {
    /// `BB_REPORT=<file saved from /api/compare> cargo test -- --nocapture` prints the digest.
    #[test]
    fn digest_prints() {
        let Ok(path) = std::env::var("BB_REPORT") else { return };
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        for (mode, _) in super::FILL_MODES {
            let r = super::digest(&v, mode).expect("digest");
            println!("== {mode}: {} {:.1} s, lag {}, counts {:?}, damage {:?}", r.log, r.secs, super::sgn(r.lag_end), r.counts, r.damage);
            for p in r.parts.iter().take(if mode == "budget" { 99 } else { 4 }) {
                let secs: Vec<String> = p.secs.iter().map(|s| format!("{} {} [{}]", s.label, s.text,
                    s.chips.iter().filter(|c| c.problem()).map(|c| format!("{}:{}{:?}", c.name, c.kind, c.flags)).collect::<Vec<_>>().join(" "))).collect();
                println!("{:>2} {:<6} {:>6} {:>6} | {}", p.num, p.name, super::sgn(p.delta), super::sgn(p.lag_end), secs.join(" | "));
            }
        }
    }
}

#[cfg(test)]
mod http_tests {
    /// `BB_CHECKER=1 cargo test` asks the running checker on 8771.
    #[test]
    fn talks_to_the_checker() {
        if std::env::var("BB_CHECKER").is_err() {
            return;
        }
        let b = super::get("127.0.0.1:8771", "/api/latest").expect("latest");
        assert!(b.contains("latest"), "{b}");
        let h = super::get("127.0.0.1:8771", "/api/history").expect("history");
        assert!(serde_json::from_str::<serde_json::Value>(&h).unwrap().is_array());
    }
}
