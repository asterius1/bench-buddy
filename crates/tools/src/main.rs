//! Bench Buddy's measuring tools.
//!
//! ```text
//! cargo run --release -p bbtool -- <tool> [args]
//! ```
//!
//! ```text
//! whatlog [count]               what your newest golem logs hold (elite spec, most cast skills)
//! feedcheck [SESSION_DIR]       how much of a pull the live feeds carry, against its log
//! livereplay [SESSION_DIR]      a recorded feed judged live, next to the log of the same pull
//! livecast [SESSION_DIR]        measures the live cast table (core/livecasts.json) from every
//!                               golem log, and checks it on a feed recording
//! livetest [STEM...] [--import [DIR]] [--save]
//!                               the live path against the log for every pull with a capture
//! ```
//!
//! Each tool's own documentation is at the top of its file in crates/tools/src/.

mod feed;
mod feedcheck;
mod livecast;
mod livereplay;
mod livetest;
mod py;
mod whatlog;

use std::path::{Path, PathBuf};

use bb_app::{App, Caches};

/// The repository: the folder with crates/ and modules/ above the current folder or the program.
fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let exe = std::env::current_exe().ok();
    [Some(cwd.clone()), exe].into_iter().flatten()
        .flat_map(|p| p.ancestors().map(Path::to_path_buf).collect::<Vec<_>>())
        .find(|p| p.join("modules").is_dir() && p.join("crates").is_dir())
        .unwrap_or(cwd)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let root = repo_root();
    let app = || App::new(Caches::Repo(root.clone()));
    let rest = args.get(1..).unwrap_or(&[]);
    let r = match args.first().map(String::as_str) {
        Some("whatlog") => whatlog::run(rest).map(|_| 0),
        Some("feedcheck") => feedcheck::run(rest).map(|_| 0),
        Some("livereplay") => livereplay::run(rest, &app()).map(|_| 0),
        Some("livecast") => livecast::run(rest, &root).map(|_| 0),
        Some("livetest") => livetest::run(rest, &app()),
        _ => Err("usage: bbtool whatlog | feedcheck | livereplay | livecast | livetest  (see crates/tools/src/main.rs)".into()),
    };
    match r {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("bbtool: {e}");
            std::process::exit(1);
        }
    }
}
