//! Package manager abstraction. Every manager is driven through its CLI;
//! Zito only parses output and builds commands. Nothing is installed or
//! removed by Zito itself.

pub mod aur;
pub mod cargo;
pub mod flatpak;
pub mod npm;
pub mod pacman;

use crate::config::Config;
use crate::exec;
use crate::model::*;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::sync::Arc;

pub trait Manager: Send + Sync {
    fn kind(&self) -> ManagerKind;
    fn binary(&self) -> String;

    fn version(&self) -> Option<String> {
        let out = exec::capture(&self.binary(), &["--version"]).ok()?;
        out.lines().map(str::trim).find(|l| !l.is_empty()).map(String::from)
    }

    /// Is the manager actually usable (not merely present)?
    fn probe(&self) -> Result<()> {
        Ok(())
    }

    fn search(&self, query: &str) -> Result<Vec<PackageEntry>>;
    fn installed(&self) -> Result<Vec<PackageEntry>>;

    fn supports_updates(&self) -> bool {
        true
    }
    fn updates(&self) -> Result<Vec<UpdateInfo>>;

    /// Package ids the manager considers orphaned (potentially unused).
    fn orphans(&self) -> Result<Vec<String>> {
        Ok(vec![])
    }

    fn info(&self, entry: &PackageEntry) -> Result<Vec<(String, String)>>;
    fn plan(&self, op: Op, entry: Option<&PackageEntry>) -> Result<CommandSpec>;
}

/// Detected / enabled / operational are tracked separately (spec 12).
#[derive(Debug, Clone)]
pub struct ManagerStatus {
    pub kind: ManagerKind,
    pub binary: String,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub operational: bool,
    pub enabled: bool,
    pub note: Option<String>,
}

impl ManagerStatus {
    pub fn eligible(&self) -> bool {
        self.enabled && self.operational
    }

    pub fn state(&self) -> &'static str {
        if self.path.is_none() {
            "not found"
        } else if !self.operational {
            "not operational"
        } else if !self.enabled {
            "disabled"
        } else {
            "ready"
        }
    }
}

pub fn detect(m: &dyn Manager, cfg: &Config) -> ManagerStatus {
    let binary = m.binary();
    let path = exec::which(&binary);
    let (version, operational, note) = if path.is_some() {
        match (m.version(), m.probe()) {
            (Some(v), Ok(())) => (Some(v), true, None),
            (v, Err(e)) => (v, false, Some(e.to_string())),
            (None, Ok(())) => (None, false, Some("version check failed".to_string())),
        }
    } else {
        (None, false, Some(format!("`{binary}` not found in PATH")))
    };
    ManagerStatus {
        kind: m.kind(),
        binary,
        path,
        version,
        operational,
        enabled: !cfg.disabled.contains(&m.kind()),
        note,
    }
}

pub fn build(cfg: &Config) -> Vec<Arc<dyn Manager>> {
    let helper = cfg
        .aur_helper
        .clone()
        .or_else(|| {
            ["yay", "paru"]
                .iter()
                .find(|h| exec::which(h).is_some())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "yay".to_string());
    let v: Vec<Arc<dyn Manager>> = vec![
        Arc::new(pacman::Pacman),
        Arc::new(aur::Aur { helper }),
        Arc::new(flatpak::Flatpak),
        Arc::new(npm::Npm),
        Arc::new(cargo::Cargo),
    ];
    v
}

// ---------- shared helpers ----------

pub fn need(e: Option<&PackageEntry>) -> Result<&PackageEntry> {
    e.context("no package selected")
}

pub fn ne(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Parses "name old -> new" lines (pacman -Qu, checkupdates, yay/paru -Qua).
pub fn parse_updates(out: &str) -> Vec<UpdateInfo> {
    out.lines()
        .filter_map(|l| {
            let t: Vec<&str> = l.split_whitespace().collect();
            (t.len() >= 4 && t[2] == "->").then(|| UpdateInfo {
                id: t[0].to_string(),
                from: t[1].to_string(),
                to: t[3].to_string(),
            })
        })
        .collect()
}

/// Generic "Key: value" lines (flatpak info, cargo info). Skips long "keys".
pub fn parse_kv(out: &str) -> Vec<(String, String)> {
    out.lines()
        .filter_map(|l| {
            let (k, v) = l.trim().split_once(':')?;
            let (k, v) = (k.trim(), v.trim());
            (!k.is_empty() && k.len() <= 24 && !v.is_empty())
                .then(|| (k.to_string(), v.to_string()))
        })
        .collect()
}
