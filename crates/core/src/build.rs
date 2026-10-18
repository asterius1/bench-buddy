//! The simple contract: what the checker and the addon ask of a build (crates/template is the
//! starting point). The Berserker and the Scrapper predate it and have their own routes in crates/app.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::rot::Record;

/// A build that follows the simple contract: served at /<route>, /api/<route>/latest, /history and
/// /compare?log=.
pub trait Build: Send + Sync {
    fn name(&self) -> &str;
    fn route(&self) -> &str;
    /// the elite spec id in arcdps, for live judging
    fn elite(&self) -> u32;
    /// the page, in modules/<route>/
    fn page(&self) -> &str {
        "page.html"
    }
    fn logs(&self) -> Vec<PathBuf>;
    /// one pull against the bench, as JSON for the page; `path` is None for a live pull
    fn compare(&self, rec: &Record, path: Option<&Path>) -> Result<Option<Value>, String>;
    /// one row per pull for the picker; None: just the logs
    fn history(&self) -> Option<Vec<Value>> {
        None
    }
    /// the live record adjusted before compare() (the Berserker puts back its weapon swaps)
    fn live(&self, rec: Record) -> Record {
        rec
    }
}

