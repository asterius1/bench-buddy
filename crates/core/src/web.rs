//! Downloads: bench logs from dps.report and skill details from the GW2 API (`core/web.py`).
//! dps.report refuses requests without a User-Agent (403), so every download goes through get().

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

const AGENT: &str = "bench-buddy (Guild Wars 2 golem rotation checker)";

pub fn get(url: &str, timeout_s: u64) -> Result<String, String> {
    ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout_s)).user_agent(AGENT).build()
        .get(url).call().map_err(|e| e.to_string())?
        .into_string().map_err(|e| e.to_string())
}

/// A dps.report log's Elite Insights JSON (permalink: the part after dps.report/), downloaded to
/// `dest` the first time and read from there afterwards.
pub fn bench_log(permalink: &str, dest: &Path) -> Result<Value, String> {
    if !dest.exists() {
        let body = get(&format!("https://dps.report/getJson?permalink={permalink}"), 60)?;
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(dest, &body).map_err(|e| e.to_string())?;
    }
    let s = std::fs::read_to_string(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    serde_json::from_str(&s).map_err(|e| format!("{}: {e}", dest.display()))
}

/// Skill details (name, icon, recharge ...) from the GW2 API, for a list of skill ids.
pub fn skills(ids: &[i64]) -> Result<Value, String> {
    let ids: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
    let body = get(&format!("https://api.guildwars2.com/v2/skills?ids={}", ids.join(",")), 20)?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}
