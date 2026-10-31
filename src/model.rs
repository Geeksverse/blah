//! Core domain types shared across MPM.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ManagerKind {
    Pacman,
    Aur,
    Flatpak,
    Npm,
    Cargo,
}

impl ManagerKind {
    pub const ALL: [ManagerKind; 5] = [
        ManagerKind::Pacman,
        ManagerKind::Aur,
        ManagerKind::Flatpak,
        ManagerKind::Npm,
        ManagerKind::Cargo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ManagerKind::Pacman => "pacman",
            ManagerKind::Aur => "aur",
            ManagerKind::Flatpak => "flatpak",
            ManagerKind::Npm => "npm",
            ManagerKind::Cargo => "cargo",
        }
    }

    /// Only desktop-application sources may be grouped under one software
    /// identity. Language-ecosystem packages (npm, cargo) are never merged
    /// with system packages: same name does not imply same software.
    pub fn groupable(self) -> bool {
        matches!(self, ManagerKind::Pacman | ManagerKind::Aur | ManagerKind::Flatpak)
    }
}

impl fmt::Display for ManagerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One concrete package from one concrete manager. Never merged or rewritten.
#[derive(Debug, Clone)]
pub struct PackageEntry {
    pub manager: ManagerKind,
    pub id: String,
    pub name: String,
    pub description: String,
    /// Available version (search results).
    pub version: Option<String>,
    /// Present only if installed.
    pub installed_version: Option<String>,
    pub repo: Option<String>,
    /// Some(false) = installed as a dependency. None = manager doesn't say.
    pub explicit: Option<bool>,
}

impl PackageEntry {
    pub fn new(manager: ManagerKind, id: impl Into<String>) -> Self {
        let id = id.into();
        Self {
            manager,
            name: id.clone(),
            id,
            description: String::new(),
            version: None,
            installed_version: None,
            repo: None,
            explicit: None,
        }
    }

    pub fn is_installed(&self) -> bool {
        self.installed_version.is_some()
    }
}

/// A conceptual software product with one or more package entries.
/// `entries` is sorted by source priority; `entries[0]` is the default source.
#[derive(Debug, Clone)]
pub struct SoftwareIdentity {
    pub name: String,
    pub description: String,
    pub entries: Vec<PackageEntry>,
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub id: String,
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Install,
    Remove,
    Upgrade,
    UpgradeAll,
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Op::Install => "install",
            Op::Remove => "remove",
            Op::Upgrade => "upgrade",
            Op::UpgradeAll => "upgrade-all",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpStatus {
    Pending,
    Running,
    Successful,
    Failed,
    Cancelled,
    PartiallySuccessful,
}

impl fmt::Display for OpStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            OpStatus::Pending => "pending",
            OpStatus::Running => "running",
            OpStatus::Successful => "successful",
            OpStatus::Failed => "failed",
            OpStatus::Cancelled => "cancelled",
            OpStatus::PartiallySuccessful => "partial",
        })
    }
}

/// A command MPM will hand to the responsible package manager.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub root: bool,
    pub note: Option<String>,
}

impl CommandSpec {
    pub fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            root: false,
            note: None,
        }
    }

    pub fn root(mut self) -> Self {
        self.root = true;
        self
    }

    pub fn note(mut self, n: &str) -> Self {
        self.note = Some(n.to_string());
        self
    }

    /// Full argv, prefixed with `sudo` only when root is required and we aren't root.
    pub fn argv(&self, is_root: bool) -> Vec<String> {
        let mut v = Vec::new();
        if self.root && !is_root {
            v.push("sudo".to_string());
        }
        v.push(self.program.clone());
        v.extend(self.args.iter().cloned());
        v
    }

    pub fn display(&self) -> String {
        self.argv(crate::exec::is_root()).join(" ")
    }
}
