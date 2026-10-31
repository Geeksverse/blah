use crate::model::ManagerKind;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Highest priority first.
    pub source_priority: Vec<ManagerKind>,
    pub disabled: Vec<ManagerKind>,
    pub aur_helper: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source_priority: ManagerKind::ALL.to_vec(),
            disabled: vec![],
            aur_helper: None,
        }
    }
}

impl Config {
    pub fn load() -> (Self, Option<String>) {
        let Some(path) = config_path() else { return (Self::default(), None) };
        match std::fs::read_to_string(&path) {
            Err(_) => (Self::default(), None), // no file: defaults
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(c) => (c, None),
                Err(e) => (Self::default(), Some(format!("{}: {e}", path.display()))),
            },
        }
    }

    /// Lower = higher priority. Managers missing from the list go last.
    pub fn rank(&self, kind: ManagerKind) -> usize {
        self.source_priority
            .iter()
            .position(|k| *k == kind)
            .unwrap_or(self.source_priority.len() + kind as usize)
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join("mpm").join("config.toml"))
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mpm")
}
