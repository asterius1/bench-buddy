//! The live pull: your events on the arcdps area feed (hits, condition ticks, buff changes) since the
//! pull started, kept as they are for the live judging, and a recording of the feed for checking it
//! against the log afterwards.
//!
//! The feed carries hits but no casts. `bb_core::live` puts the casts back from the hits and the
//! build's own crate judges them (`bb_app::App::live`), so nothing here knows a build.

use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{BufWriter, Write},
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

/// A gap in your hits this long starts a new pull.
const NEW_PULL_MS: u64 = 12_000;
/// The same after the pull's log came in: hits still arriving then are the old pull's (the feed
/// is 2.7 s late), a pause after them starts the next one.
const ENDED_GAP_MS: u64 = 3_000;
/// Kept from before a pull's first hit: buffs you had, casts already under way.
const BEFORE_MS: u64 = 3_000;

#[derive(Default)]
pub struct Live {
    /// the pull's events as the judge reads them, oldest first
    events: Vec<(u64, Value)>,
    /// the pull's direct hits so far
    pub hits: usize,
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
    /// The pull's log has come in.
    pub fn end_pull(&mut self) {
        self.ended = true;
    }

    /// One event of yours. Only what the judge reads is kept: direct hits and condition ticks (state
    /// change 0), initial buffs (18), buffs applied (69) and gone (72). Direct hits come on both
    /// feeds: the local copy within 20 ms, the area copy 2.6 s later with the exact time (the local
    /// one is off by up to 40 ms and its value negative). So a local hit is taken at once and its
    /// area copy, when it comes, takes its place; everything else comes from the area feed only.
    pub fn event(&mut self, feed: &str, e: &nexus::event::arc::evtc::Event, dst_self: bool) {
        let sc = e.is_statechange as u8;
        if !matches!(sc, 0 | 18 | 69 | 72) || (sc == 0 && e.is_activation != 0) {
            return;
        }
        let direct = sc == 0 && e.buff == 0 && e.is_buffremove == 0 && e.dst_agent != 0 && !dst_self;
        let local = feed == "local";
        if local && !direct {
            return;
        }
        if direct && !local {
            // the area copy of a hit already taken from the local feed: it only corrects the time
            let twin = self.events.iter().position(|(t, v)| {
                v["local"] == true && v["skill"] == e.skill_id && v["value"] == e.value.unsigned_abs() && t.abs_diff(e.time) <= 60
            });
            if let Some(i) = twin {
                let (_, mut v) = self.events.remove(i);
                v["time"] = e.time.into();
                v["local"] = false.into();
                let at = self.events.partition_point(|(t, _)| *t <= e.time);
                self.events.insert(at, (e.time, v));
                self.version += 1;
                return;
            }
        }
        if direct {
            // once the pull's log is in, a short pause is enough to tell the next pull
            let gap = if self.ended { ENDED_GAP_MS } else { NEW_PULL_MS };
            if self.last_hit != 0 && e.time > self.last_hit + gap {
                let keep: Vec<(u64, Value)> = self.events.drain(..).filter(|(t, _)| *t + BEFORE_MS >= e.time).collect();
                *self = Live { version: self.version + 1, events: keep, ..Live::default() };
            }
            if self.first_hit == 0 {
                self.first_hit = e.time;
            }
            self.last_hit = self.last_hit.max(e.time);
            self.last_arrival = Some(Instant::now());
            self.hits += 1;
        } else if self.first_hit == 0 || self.ended {
            // before a pull: only the last few seconds matter
            self.events.retain(|(t, _)| *t + BEFORE_MS >= e.time);
        }
        let v = json!({"time": e.time, "sc": sc, "buff": e.buff, "skill": e.skill_id, "value": e.value.unsigned_abs(),
                       "buff_dmg": e.buff_dmg, "result": e.result, "buffremove": e.is_buffremove,
                       "dst": e.dst_agent, "dst_self": dst_self, "local": local});
        let at = self.events.partition_point(|(t, _)| *t <= e.time);
        self.events.insert(at, (e.time, v));
        self.version += 1;
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

    /// What `bb_app::App::live` judges.
    pub fn to_json(&self, elite: u32) -> Value {
        json!({"elite": elite, "events": self.events.iter().map(|(_, v)| v).collect::<Vec<_>>()})
    }
}

/// Every event of yours on both feeds, as JSON Lines in the format `bbtool feedcheck` reads.
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
