//! MPM's own operation history (JSON Lines), independent of manager logs.

use crate::config::data_dir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub ts: u64,
    pub op: String,
    pub manager: String,
    pub package: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub status: String,
    pub detail: Option<String>,
}

fn path() -> PathBuf {
    data_dir().join("history.jsonl")
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn append(rec: &Record) -> Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
    writeln!(f, "{}", serde_json::to_string(rec)?)?;
    Ok(())
}

/// Newest first.
pub fn load(limit: usize) -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(path()) else { return vec![] };
    let mut v: Vec<Record> = text
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    v.reverse();
    v.truncate(limit);
    v
}

/// UTC "YYYY-MM-DD HH:MM" without pulling in a date crate.
pub fn fmt_ts(ts: u64) -> String {
    let days = (ts / 86_400) as i64;
    let secs = ts % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}
