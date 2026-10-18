//! What your newest golem logs hold: the settings a new build needs (its elite spec and signature).
//!
//!     bbtool whatlog [count]        (default the newest 5 logs)
//!
//! For each log: your character, its elite spec id and your most cast skills with their ids, so you
//! can pick a signature: one or two skill ids that only your build casts.

use bb_core::omap::OMap;
use bb_core::rot;

use crate::py::most_common;

pub fn run(args: &[String]) -> Result<(), String> {
    let n: usize = args.first().and_then(|a| a.parse().ok()).unwrap_or(5);
    let all = rot::all_golem_logs();
    let paths = &all[all.len().saturating_sub(n)..];
    if paths.is_empty() {
        return Err(format!("no golem logs in {} (set BENCH_BUDDY_LOGS if arcdps writes them elsewhere)", rot::logs_dir().display()));
    }
    for p in paths {
        let r = match rot::read(p) {
            Ok(r) => r,
            Err(e) => {
                println!("{} unreadable: {e}", rot::stem(p));
                continue;
            }
        };
        let mut c: OMap<(i64, String), i64> = OMap::default();
        for x in r.casts.iter().filter(|c| c.dur.unwrap_or(0) >= 100) {
            *c.entry_or((x.id, x.name.clone()), 0) += 1;
        }
        println!("{}  {}  ELITE = {}  ({:.0} s, {} casts)", rot::stem(p), r.player, r.elite, r.end as f64 / 1000.0, r.casts.len());
        for ((sid, name), k) in most_common(&c).into_iter().take(10) {
            println!("    {sid:>6}  {name:<30} x{k}");
        }
        if r.casts.is_empty() {
            println!("    no casts: logs before October 2026 carry no animation events");
        }
    }
    Ok(())
}
