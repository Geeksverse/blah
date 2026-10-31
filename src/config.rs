use crate::identity::normalize;
use crate::model::ManagerKind;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Highest priority first.
    pub source_priority: Vec<ManagerKind>,
    pub disabled: Vec<ManagerKind>,
    pub aur_helper: Option<String>,
    /// Explicit "same software" mappings (alias name -> canonical name). Only
    /// applied to application sources (pacman / aur / flatpak).
    pub aliases: BTreeMap<String, String>,
    /// Per-software default source (software name -> manager). Only picks the
    /// default; never installs, removes or migrates anything.
    pub preferred_source: BTreeMap<String, ManagerKind>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            source_priority: ManagerKind::ALL.to_vec(),
            disabled: vec![],
            aur_helper: None,
            aliases: BTreeMap::new(),
            preferred_source: BTreeMap::new(),
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

    /// Normalized name after applying an explicit alias (one hop).
    pub fn canonical_name(&self, name: &str) -> String {
        let n = normalize(name);
        self.aliases
            .iter()
            .find(|(k, _)| normalize(k) == n)
            .map(|(_, v)| normalize(v))
            .unwrap_or(n)
    }

    /// Configured default source for a software, keyed by canonical name.
    pub fn preferred_for(&self, canonical: &str) -> Option<ManagerKind> {
        self.preferred_source
            .iter()
            .find(|(k, _)| self.canonical_name(k) == canonical)
            .map(|(_, v)| *v)
    }

    /// Non-fatal problems in an otherwise valid config. Reported to the user;
    /// never silently "fixed".
    pub fn warnings(&self) -> Vec<String> {
        let mut w = Vec::new();
        for (i, k) in self.source_priority.iter().enumerate() {
            if self.source_priority[..i].contains(k) {
                w.push(format!("source_priority lists {k} more than once (first entry wins)"));
            }
        }
        for (name, k) in &self.preferred_source {
            if self.disabled.contains(k) {
                w.push(format!("preferred_source for “{name}” is {k}, which is disabled"));
            }
        }
        w
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".config")))?;
    Some(base.join("zito").join("config.toml"))
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| home().map(|h| h.join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("zito")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_follows_priority_and_unlisted_go_last() {
        let mut c = Config::default();
        c.source_priority = vec![ManagerKind::Flatpak, ManagerKind::Pacman];
        assert!(c.rank(ManagerKind::Flatpak) < c.rank(ManagerKind::Pacman));
        assert!(c.rank(ManagerKind::Pacman) < c.rank(ManagerKind::Aur));
        // Unlisted managers keep a stable relative order.
        assert!(c.rank(ManagerKind::Aur) < c.rank(ManagerKind::Npm));
    }

    #[test]
    fn alias_lookup_is_normalized_and_single_hop() {
        let mut c = Config::default();
        c.aliases.insert("Visual Studio Code".into(), "code".into());
        c.aliases.insert("code".into(), "other".into());
        assert_eq!(c.canonical_name("visual  studio code"), "code");
        assert_eq!(c.canonical_name("unrelated"), "unrelated");
    }

    #[test]
    fn warnings_flag_duplicates_and_disabled_preferences() {
        let mut c = Config::default();
        c.source_priority = vec![ManagerKind::Pacman, ManagerKind::Pacman];
        c.disabled = vec![ManagerKind::Flatpak];
        c.preferred_source.insert("firefox".into(), ManagerKind::Flatpak);
        assert_eq!(c.warnings().len(), 2);
        assert!(Config::default().warnings().is_empty());
    }
}
