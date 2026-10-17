//! The live pull: your casts put back from your hits on the arcdps area feed (which carries hits
//! but no casts), and a recording of that feed for checking it against the log afterwards.
//!
//! A cast starts `offset` ms before its skill's first hit; further hits of that skill within
//! `window` ms belong to it, and for a pulsing field (Scorched Earth) only those on its beat.
//! Flames of War and Blood Reckoning do no damage: they start `offset` ms before the buff they put
//! on you. The table is `casts.json`, measured from the golem logs by `tools/livecast.py`.
//! `core/live_server.py` judges the result with the checker's own code.

use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const PULSE_TOL: i64 = 60;
/// A gap in your hits this long starts a new pull.
const NEW_PULL_MS: u64 = 12_000;
/// The same after the pull's log came in: hits still arriving then are the old pull's (the feed
/// is 2.7 s late), a pause after them starts the next one.
const ENDED_GAP_MS: u64 = 3_000;

pub struct Rule {
    pub name: String,
    offset: i64,
    window: i64,
    pulse: i64,
    /// the buff that shows this skill live, for skills that do not hit
    buff: Option<u32>,
}

pub fn rules() -> HashMap<u32, Rule> {
    let v: Value = serde_json::from_str(include_str!("../casts.json")).expect("casts.json");
    v.as_object()
        .unwrap()
        .iter()
        .filter_map(|(k, r)| {
            Some((
                k.parse().ok()?,
                Rule {
                    name: r["name"].as_str()?.to_string(),
                    offset: r["offset"].as_i64()?,
                    window: r["window"].as_i64()?,
                    pulse: r["pulse"].as_i64().unwrap_or(0),
                    buff: r["buff"].as_u64().map(|b| b as u32),
                },
            ))
        })
        .collect()
}

pub struct Cast {
    pub skill: u32,
    /// arcdps time (ms) of the guessed cast start
    pub start: u64,
    pub hits: u32,
}

#[derive(Default)]
pub struct Live {
    pub casts: Vec<Cast>,
    /// per skill, the first hits of its casts that can still be hitting, with their start
    open: HashMap<u32, Vec<(u64, u64)>>,
    buffs: HashMap<u32, Vec<u64>>,
    /// the pull's first hit (arcdps starts its log there) and newest hit
    first_hit: u64,
    last_hit: u64,
    /// when the newest hit arrived, to tell a pull in progress
    pub last_arrival: Option<Instant>,
    /// bumped on every change, so the judge is only asked again when something is new
    pub version: u64,
    /// the log of this pull has come in
    ended: bool,
}

impl Live {
    fn new_pull_check(&mut self, time: u64) {
        // once the pull's log is in, a short pause is enough to tell the next pull
        let gap = if self.ended { ENDED_GAP_MS } else { NEW_PULL_MS };
        if self.last_hit != 0 && time > self.last_hit + gap {
            *self = Live { version: self.version + 1, ..Live::default() };
        }
    }

    /// The pull's log has come in.
    pub fn end_pull(&mut self) {
        self.ended = true;
    }

    fn add(&mut self, skill: u32, start: u64, hits: u32) {
        let at = self.casts.partition_point(|c| c.start <= start);
        self.casts.insert(at, Cast { skill, start, hits });
        self.version += 1;
    }

    pub fn hit(&mut self, time: u64, skill: u32, rules: &HashMap<u32, Rule>) {
        self.new_pull_check(time);
        if self.first_hit == 0 {
            self.first_hit = time;
        }
        self.last_hit = self.last_hit.max(time);
        self.last_arrival = Some(Instant::now());
        let Some(r) = rules.get(&skill).filter(|r| r.buff.is_none()) else { return };
        let open = self.open.entry(skill).or_default();
        open.retain(|&(f, _)| (time as i64 - f as i64) <= r.window);
        let same = open.iter().find(|&&(f, _)| {
            let d = time as i64 - f as i64;
            r.pulse == 0 || ((d + PULSE_TOL).rem_euclid(r.pulse) - PULSE_TOL).abs() <= PULSE_TOL
        });
        if let Some(&(_, start)) = same {
            // a further hit of a cast already seen; a field's pulses are not the cast's hits
            if r.pulse == 0 {
                if let Some(c) = self.casts.iter_mut().find(|c| c.skill == skill && c.start == start) {
                    c.hits += 1;
                    self.version += 1;
                }
            }
            return;
        }
        let start = (time as i64 - r.offset).max(0) as u64;
        open.push((time, start));
        self.add(skill, start, 1);
    }

    pub fn buff(&mut self, time: u64, buff: u32, rules: &HashMap<u32, Rule>) {
        let Some((&skill, r)) = rules.iter().find(|(_, r)| r.buff == Some(buff)) else { return };
        self.new_pull_check(time);
        self.buffs.entry(buff).or_default().push(time);
        self.add(skill, (time as i64 - r.offset).max(0) as u64, 0);
    }

    pub fn last_hit(&self) -> u64 {
        self.last_hit
    }

    pub fn first_hit(&self) -> u64 {
        self.first_hit
    }

    pub fn in_pull(&self) -> bool {
        self.last_arrival.map_or(false, |t| t.elapsed().as_millis() < 15_000)
    }

    /// What `core/live_server.py` judges.
    pub fn to_json(&self) -> Value {
        json!({
            "casts": self.casts.iter().map(|c| json!({"id": c.skill, "t": c.start, "hits": c.hits})).collect::<Vec<_>>(),
            "buffs": self.buffs.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<_, _>>(),
            "start": self.first_hit,
            "end": self.last_hit,
        })
    }
}

/// Every event of yours on both feeds, as JSON Lines in the format `tools/feedcheck.py` reads.
pub struct Recorder {
    out: BufWriter<File>,
    t0: Instant,
    last_flush: Instant,
}

impl Recorder {
    pub fn start(dir: &Path) -> std::io::Result<Self> {
        let d = dir.join("sessions").join(stamp());
        fs::create_dir_all(&d)?;
        let out = BufWriter::new(File::create(d.join("combat.jsonl"))?);
        fs::write(
            d.join("meta.json"),
            json!({"clock": "t = microseconds since session start", "writer": "bench_buddy"}).to_string(),
        )?;
        let now = Instant::now();
        Ok(Self { out, t0: now, last_flush: now })
    }

    pub fn now_us(&self) -> i128 {
        self.t0.elapsed().as_micros() as i128
    }

    pub fn write(&mut self, feed: &str, e: &nexus::event::arc::evtc::Event, t: i128) {
        let line = json!({
            "feed": feed, "t": t as i64,
            "ev": {"time": e.time, "is_statechange": e.is_statechange, "buff": e.buff,
                   "is_buffremove": e.is_buffremove, "is_activation": e.is_activation,
                   "skill_id": e.skill_id, "value": e.value, "buff_dmg": e.buff_dmg,
                   "result": e.result, "dst_agent": e.dst_agent},
            "src": {"self": 1},
        });
        let _ = writeln!(self.out, "{line}");
        if self.last_flush.elapsed().as_secs() >= 2 {
            let _ = self.out.flush();
            self.last_flush = Instant::now();
        }
    }

    pub fn flush(&mut self) {
        let _ = self.out.flush();
    }
}

/// UTC "YYYYMMDD-HHMMSS".
fn stamp() -> String {
    let s = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    // civil from days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}{m:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}
