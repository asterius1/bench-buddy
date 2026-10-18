//! Minimal raw arcdps `.zevtc` reader: agents, skill names and every combat event.

use std::collections::HashMap;
use std::path::Path;

pub struct Agent {
    pub addr: u64,
    pub prof: u32,
    pub elite: u32,
    pub name: String,
    pub is_player: bool,
    /// The species id of an NPC, None for a player.
    pub species: Option<u32>,
}

/// One 64-byte combat event; only the fields the judging reads.
#[derive(Clone, Copy, Default)]
pub struct Event {
    pub time: u64,
    pub src: u64,
    pub dst: u64,
    pub value: i32,
    pub buff_dmg: i32,
    pub skill: u32,
    pub buff: u8,
    pub result: u8,
    pub activation: u8,
    pub buffremove: u8,
    pub sc: u8,
}

pub struct Log {
    /// In the order of the log's agent table.
    pub agents: Vec<Agent>,
    pub by_addr: HashMap<u64, usize>,
    pub skills: HashMap<i64, String>,
    pub events: Vec<Event>,
}

impl Log {
    pub fn agent(&self, addr: u64) -> Option<&Agent> {
        self.by_addr.get(&addr).map(|&i| &self.agents[i])
    }
}

fn u16_at(d: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([d[o], d[o + 1]])
}
fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}
fn u64_at(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}

fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

/// The first file of a zip archive, read through its central directory.
fn unzip_first(z: &[u8]) -> Result<Vec<u8>, String> {
    unzip_first_upto(z, usize::MAX)
}

/// The same, stopping after `limit` bytes.
fn unzip_first_upto(z: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let eocd = (0..z.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(z, i) == 0x0605_4b50)
        .ok_or("not a zip file")?;
    let cd = u32_at(z, eocd + 16) as usize;
    if cd + 46 > z.len() || u32_at(z, cd) != 0x0201_4b50 {
        return Err("bad zip directory".into());
    }
    let method = u16_at(z, cd + 10);
    let size = u32_at(z, cd + 20) as usize;
    let local = u32_at(z, cd + 42) as usize;
    if local + 30 > z.len() || u32_at(z, local) != 0x0403_4b50 {
        return Err("bad zip entry".into());
    }
    let start = local + 30 + u16_at(z, local + 26) as usize + u16_at(z, local + 28) as usize;
    let data = z.get(start..start + size).ok_or("truncated zip")?;
    match method {
        0 => Ok(data.to_vec()),
        8 if limit == usize::MAX => miniz_oxide::inflate::decompress_to_vec(data).map_err(|e| format!("inflate: {e:?}")),
        8 => match miniz_oxide::inflate::decompress_to_vec_with_limit(data, limit) {
            Ok(v) => Ok(v),
            Err(e) if e.output.len() >= limit.min(16) => Ok(e.output),
            Err(e) => Err(format!("inflate: {:?}", e.status)),
        },
        m => Err(format!("zip method {m}")),
    }
}

/// The boss species of a log from its header (bytes 13-14), without reading the rest: 16199 for the
/// Standard Kitty Golem. This finds golem logs whatever folder arcdps put them in.
pub fn species(path: &Path) -> Option<u16> {
    let z = std::fs::read(path).ok()?;
    let d = unzip_first_upto(&z, 64).ok()?;
    (d.len() >= 16 && &d[..4] == b"EVTC").then(|| u16_at(&d, 13))
}

pub fn load(path: &Path) -> Result<Log, String> {
    let z = std::fs::read(path).map_err(|e| e.to_string())?;
    let d = unzip_first(&z)?;
    if d.len() < 20 {
        return Err("short log".into());
    }
    let mut o = 16;
    let n = u32_at(&d, o) as usize;
    o += 4;
    let mut agents = Vec::with_capacity(n);
    let mut by_addr = HashMap::new();
    for _ in 0..n {
        if o + 96 > d.len() {
            return Err("truncated agents".into());
        }
        let (addr, prof, elite) = (u64_at(&d, o), u32_at(&d, o + 8), u32_at(&d, o + 12));
        let is_player = elite != 0xFFFF_FFFF;
        // a later agent with the same address replaces the earlier one
        by_addr.insert(addr, agents.len());
        agents.push(Agent { addr, prof, elite, name: cstr(&d[o + 28..o + 92]), is_player,
                            species: if is_player { None } else { Some(prof & 0xFFFF) } });
        o += 96;
    }
    let n = u32_at(&d, o) as usize;
    o += 4;
    let mut skills = HashMap::new();
    for _ in 0..n {
        if o + 68 > d.len() {
            return Err("truncated skills".into());
        }
        let sid = u32_at(&d, o) as i32 as i64;
        skills.insert(sid, cstr(&d[o + 4..o + 68]));
        o += 68;
    }
    let mut events = Vec::with_capacity(d.len().saturating_sub(o) / 64);
    let mut b = o;
    while b + 64 <= d.len() {
        events.push(Event {
            time: u64_at(&d, b),
            src: u64_at(&d, b + 8),
            dst: u64_at(&d, b + 16),
            value: u32_at(&d, b + 24) as i32,
            buff_dmg: u32_at(&d, b + 28) as i32,
            skill: u32_at(&d, b + 36),
            buff: d[b + 49],
            result: d[b + 50],
            activation: d[b + 51],
            buffremove: d[b + 52],
            sc: d[b + 56],
        });
        b += 64;
    }
    Ok(Log { agents, by_addr, skills, events })
}
