//! The judging from the command line, for checking it against the Python checker.
//!
//!     bbjudge logs                      the Scrapper golem logs, oldest first
//!     bbjudge compare <log.zevtc>       compare() as JSON
//!     bbjudge panel <log.zevtc>         the addon's panel as JSON
//!     bbjudge sheet                     the bench cheat sheet as JSON
//!     bbjudge dump <dir>                compare and panel of every Scrapper log into <dir>, timed
//!
//! The bench is modules/scrapper/cache/bench.json (from the checker's first run), or BENCH_BUDDY_BENCH.

use std::path::{Path, PathBuf};
use std::time::Instant;

use bench_judge::scrapper::{self, panel::panel, sheet::sheet, Bench};
use serde_json::{json, Value};

fn bench() -> Bench {
    let p = std::env::var_os("BENCH_BUDDY_BENCH").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("modules").join("scrapper").join("cache").join("bench.json"));
    Bench::load(&p).unwrap_or_else(|e| fail(&e))
}

fn fail(e: &str) -> ! {
    eprintln!("bbjudge: {e}");
    std::process::exit(1)
}

fn both(p: &Path, b: &Bench) -> Value {
    let c = scrapper::compare_log(p, b).unwrap_or_else(|e| fail(&format!("{}: {e}", p.display())));
    let pn = c.clone().map(panel);
    json!({"compare": c, "panel": pn})
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg = |i: usize| args.get(i).cloned().unwrap_or_else(|| fail("missing argument"));
    match args.first().map(String::as_str) {
        Some("logs") => {
            for p in scrapper::logs() {
                println!("{}", p.display());
            }
        }
        Some("compare") => {
            let c = scrapper::compare_log(Path::new(&arg(1)), &bench()).unwrap_or_else(|e| fail(&e));
            println!("{}", serde_json::to_string(&c).unwrap());
        }
        Some("panel") => println!("{}", both(Path::new(&arg(1)), &bench())["panel"]),
        Some("sheet") => println!("{}", sheet(&bench())),
        Some("dump") => {
            let dir = PathBuf::from(arg(1));
            std::fs::create_dir_all(&dir).unwrap_or_else(|e| fail(&e.to_string()));
            let t = Instant::now();
            let b = bench();
            let tb = t.elapsed();
            let t = Instant::now();
            let ls = scrapper::logs();
            let tl = t.elapsed();
            let t = Instant::now();
            for p in &ls {
                let out = both(p, &b);
                std::fs::write(dir.join(format!("{}.json", bench_judge::rot::stem(p))), out.to_string()).unwrap();
            }
            let tc = t.elapsed();
            std::fs::write(dir.join("_sheet.json"), sheet(&b).to_string()).unwrap();
            eprintln!("bench read in {:.2} s; {} logs found in {:.2} s; compare+panel all: {:.2} s",
                      tb.as_secs_f64(), ls.len(), tl.as_secs_f64(), tc.as_secs_f64());
        }
        _ => fail("usage: bbjudge logs | compare <log> | panel <log> | sheet | dump <dir>"),
    }
}
