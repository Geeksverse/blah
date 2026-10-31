//! Zito's own operation history (JSON Lines), independent of manager logs.
//!
//! Schema `v`: 1 = original fields; 2 adds `exit_code` and `duration_ms`.
//! Older lines stay readable. Malformed or partially written lines are
//! skipped, never fatal. History is an audit trail, not an undo log.

use crate::config::data_dir;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub const SCHEMA: u32 = 2;

fn v1() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    #[serde(default = "v1")]
    pub v: u32,
    pub ts: u64,
    pub op: String,
    pub manager: String,
    pub package: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub status: String,
    pub detail: Option<String>,
    /// Process exit code; None if the process never started or was killed by a signal.
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
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
    // One write call per record keeps a line from being interleaved or split.
    let mut line = serde_json::to_string(rec)?;
    line.push('\n');
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(p)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// Newest first.
pub fn load(limit: usize) -> Vec<Record> {
    let Ok(text) = std::fs::read_to_string(path()) else { return vec![] };
    parse_lines(&text, limit)
}

fn parse_lines(text: &str, limit: usize) -> Vec<Record> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(op: &str) -> Record {
        Record {
            v: SCHEMA,
            ts: 1_700_000_000,
            op: op.into(),
            manager: "pacman".into(),
            package: Some("vim".into()),
            before: None,
            after: None,
            status: "successful".into(),
            detail: None,
            exit_code: Some(0),
            duration_ms: Some(1234),
        }
    }

    #[test]
    fn record_round_trips() {
        let line = serde_json::to_string(&rec("install")).unwrap();
        let back: Record = serde_json::from_str(&line).unwrap();
        assert_eq!(back.v, SCHEMA);
        assert_eq!(back.exit_code, Some(0));
        assert_eq!(back.duration_ms, Some(1234));
    }

    #[test]
    fn v1_lines_without_new_fields_still_parse() {
        let old = r#"{"ts":5,"op":"remove","manager":"flatpak","package":"x","before":null,"after":null,"status":"failed","detail":"exit status 1"}"#;
        let r = parse_lines(old, 10);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].v, 1);
        assert_eq!(r[0].exit_code, None);
    }

    #[test]
    fn malformed_and_truncated_lines_are_skipped() {
        let good = serde_json::to_string(&rec("install")).unwrap();
        let text = format!("{good}\nnot json\n{{\"ts\":1,\"op\"\n{good}\n");
        assert_eq!(parse_lines(&text, 10).len(), 2);
    }

    #[test]
    fn newest_first_and_limited() {
        let text = ["a", "b", "c"]
            .iter()
            .map(|o| serde_json::to_string(&rec(o)).unwrap())
            .collect::<Vec<_>>()
            .join("\n");
        let r = parse_lines(&text, 2);
        assert_eq!(r.iter().map(|x| x.op.as_str()).collect::<Vec<_>>(), ["c", "b"]);
    }

    #[test]
    fn fmt_ts_known_values() {
        assert_eq!(fmt_ts(0), "1970-01-01 00:00");
        assert_eq!(fmt_ts(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(fmt_ts(951_782_400), "2000-02-29 00:00");
    }
}
