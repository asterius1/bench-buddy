//! Bench Buddy's judging in Rust, ported from the Python checker one build at a time (the Scrapper
//! first). It reads arcdps logs and returns the same JSON the Python modules give, so the pages and
//! the addon can use it unchanged. `tools/judgeparity.py` holds it against the Python checker.

pub mod difflib;
pub mod evtc;
pub mod kit;
pub mod omap;
pub mod rot;
pub mod scrapper;
