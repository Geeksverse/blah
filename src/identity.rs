//! Software identity grouping.
//!
//! Deliberately conservative: wrongly merging unrelated packages is worse than
//! showing them separately (spec 4.2). Rules:
//! - Entries from application sources (pacman / aur / flatpak) are merged only
//!   when their canonical names are equal. A canonical name is the normalized
//!   display name, or the target of an explicit user-configured alias.
//! - npm and cargo entries are never merged with anything.
//! - Similar names alone never merge. Every merge carries a `GroupReason`.
//!
//! Ranking is deterministic: relevance tier, then name, then default source
//! priority, then package id. The real matching algorithm remains an OPEN
//! requirement (spec 21); this module only implements the strong-signal subset.

use crate::config::Config;
use crate::model::{GroupReason, PackageEntry, SoftwareIdentity};
use std::collections::BTreeMap;

pub fn normalize(s: &str) -> String {
    s.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
}

/// Alphanumeric words of an already-normalized string.
fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|t| !t.is_empty()).map(str::to_string).collect()
}

pub fn group(entries: Vec<PackageEntry>, cfg: &Config, query: &str) -> Vec<SoftwareIdentity> {
    let mut map: BTreeMap<String, Vec<PackageEntry>> = BTreeMap::new();
    for e in entries {
        let key = if e.manager.groupable() {
            format!("app:{}", cfg.canonical_name(&e.name))
        } else {
            format!("{}:{}", e.manager.label(), e.id)
        };
        map.entry(key).or_default().push(e);
    }

    let mut out: Vec<SoftwareIdentity> = map
        .into_iter()
        .map(|(key, mut es)| {
            let canonical = key.strip_prefix("app:").map(str::to_string);
            let preferred = canonical.as_deref().and_then(|c| cfg.preferred_for(c));
            // Default source first: explicit per-software preference, then
            // global priority, then stable tie-breaks.
            es.sort_by_key(|e| {
                (Some(e.manager) != preferred, cfg.rank(e.manager), e.id.clone(), e.repo.clone())
            });
            let reason = match (&canonical, es.len()) {
                (Some(c), n) if n > 1 => {
                    if es.iter().any(|e| normalize(&e.name) != *c) {
                        GroupReason::Alias(c.clone())
                    } else {
                        GroupReason::ExactName(c.clone())
                    }
                }
                _ => GroupReason::Single,
            };
            let mut id = identity_from(es);
            id.reason = reason;
            id
        })
        .collect();

    let q = Query::new(query);
    out.sort_by_cached_key(|i| {
        let d = &i.entries[0];
        (relevance(i, &q, cfg), normalize(&i.name), cfg.rank(d.manager), d.id.clone())
    });
    out
}

struct Query {
    norm: String,
    lower: String,
    tokens: Vec<String>,
}

impl Query {
    fn new(q: &str) -> Self {
        let norm = normalize(q);
        Self { tokens: tokens(&norm), lower: q.trim().to_lowercase(), norm }
    }
}

/// Lower = more relevant. Tiers: exact name / exact id / exact alias (0),
/// name prefix (1), all query words present as name words (2), name substring
/// (3), id substring (4), description substring (5), anything else (6).
fn relevance(i: &SoftwareIdentity, q: &Query, cfg: &Config) -> u8 {
    let n = normalize(&i.name);
    let exact_alias =
        i.entries[0].manager.groupable() && cfg.canonical_name(&i.name) == cfg.canonical_name(&q.norm);
    if n == q.norm || exact_alias || i.entries.iter().any(|e| normalize(&e.id) == q.norm) {
        0
    } else if n.starts_with(&q.norm) {
        1
    } else if !q.tokens.is_empty() && {
        let nt = tokens(&n);
        q.tokens.iter().all(|t| nt.contains(t))
    } {
        2
    } else if n.contains(&q.norm) {
        3
    } else if i.entries.iter().any(|e| normalize(&e.id).contains(&q.norm)) {
        4
    } else if !q.lower.is_empty() && i.entries.iter().any(|e| e.description.to_lowercase().contains(&q.lower)) {
        5
    } else {
        6
    }
}

/// One identity per entry (installed / updates / orphans views).
pub fn singles(entries: Vec<PackageEntry>) -> Vec<SoftwareIdentity> {
    entries.into_iter().map(|e| identity_from(vec![e])).collect()
}

fn identity_from(es: Vec<PackageEntry>) -> SoftwareIdentity {
    let name = es[0].name.clone();
    let description = es
        .iter()
        .map(|e| e.description.as_str())
        .find(|d| !d.is_empty())
        .unwrap_or("")
        .to_string();
    SoftwareIdentity { name, description, entries: es, reason: GroupReason::Single }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ManagerKind::*;
    use crate::model::ManagerKind;

    fn entry(kind: ManagerKind, id: &str, name: &str) -> PackageEntry {
        let mut e = PackageEntry::new(kind, id);
        e.name = name.to_string();
        e
    }

    fn names(v: &[SoftwareIdentity]) -> Vec<&str> {
        v.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn normalize_trims_lowercases_and_hyphenates() {
        assert_eq!(normalize("  Visual   Studio Code "), "visual-studio-code");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn app_sources_merge_on_exact_normalized_name_only() {
        let cfg = Config::default();
        let g = group(
            vec![
                entry(Flatpak, "org.mozilla.firefox", "Firefox"),
                entry(Pacman, "firefox", "firefox"),
                entry(Pacman, "firefox-esr", "firefox-esr"),
            ],
            &cfg,
            "firefox",
        );
        assert_eq!(g.len(), 2, "similar names must not merge");
        let ff = &g[0];
        assert_eq!(ff.entries.len(), 2);
        assert_eq!(ff.entries[0].manager, Pacman, "default source follows priority");
        assert_eq!(ff.reason, GroupReason::ExactName("firefox".into()));
        assert_eq!(g[1].reason, GroupReason::Single);
    }

    #[test]
    fn npm_and_cargo_never_merge_with_anything() {
        let g = group(
            vec![
                entry(Pacman, "ripgrep", "ripgrep"),
                entry(Cargo, "ripgrep", "ripgrep"),
                entry(Npm, "ripgrep", "ripgrep"),
            ],
            &Config::default(),
            "ripgrep",
        );
        assert_eq!(g.len(), 3);
        assert!(g.iter().all(|i| i.entries.len() == 1 && i.reason == GroupReason::Single));
    }

    #[test]
    fn explicit_alias_merges_and_is_explained() {
        let mut cfg = Config::default();
        cfg.aliases.insert("Visual Studio Code".into(), "code".into());
        let g = group(
            vec![
                entry(Flatpak, "com.visualstudio.code", "Visual Studio Code"),
                entry(Pacman, "code", "code"),
            ],
            &cfg,
            "code",
        );
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].reason, GroupReason::Alias("code".into()));
        assert!(g[0].reason.explain().is_some());
    }

    #[test]
    fn preferred_source_overrides_priority_without_dropping_alternatives() {
        let mut cfg = Config::default();
        cfg.preferred_source.insert("firefox".into(), Flatpak);
        let g = group(
            vec![entry(Pacman, "firefox", "firefox"), entry(Flatpak, "org.mozilla.firefox", "Firefox")],
            &cfg,
            "firefox",
        );
        assert_eq!(g[0].entries.len(), 2);
        assert_eq!(g[0].entries[0].manager, Flatpak);
        assert_eq!(g[0].entries[1].manager, Pacman);
    }

    #[test]
    fn ranking_tiers_are_ordered() {
        let mut desc_only = entry(Pacman, "zzz", "zzz");
        desc_only.description = "A Firefox helper".into();
        let g = group(
            vec![
                desc_only,
                entry(Pacman, "superfirefoxy", "superfirefoxy"),
                entry(Pacman, "vim-firefox-bridge", "vim-firefox-bridge"),
                entry(Pacman, "firefox-developer-edition", "firefox-developer-edition"),
                entry(Pacman, "firefox", "firefox"),
            ],
            &Config::default(),
            "firefox",
        );
        assert_eq!(
            names(&g),
            ["firefox", "firefox-developer-edition", "vim-firefox-bridge", "superfirefoxy", "zzz"]
        );
    }

    #[test]
    fn exact_id_match_outranks_prefix() {
        let g = group(
            vec![
                entry(Flatpak, "org.gnome.Calculator", "Calculator"),
                entry(Pacman, "gnome-calculator-extras", "gnome-calculator-extras"),
            ],
            &Config::default(),
            "org.gnome.calculator",
        );
        assert_eq!(g[0].name, "Calculator");
    }

    #[test]
    fn ordering_is_deterministic_for_ties() {
        let input = vec![
            entry(Npm, "left-pad", "left-pad"),
            entry(Cargo, "left-pad", "left-pad"),
            entry(Pacman, "left-pad", "left-pad"),
        ];
        let a = group(input.clone(), &Config::default(), "left");
        let mut rev = input;
        rev.reverse();
        let b = group(rev, &Config::default(), "left");
        let order = |v: &[SoftwareIdentity]| v.iter().map(|i| i.entries[0].manager).collect::<Vec<_>>();
        assert_eq!(order(&a), order(&b));
        assert_eq!(order(&a), vec![Pacman, Npm, Cargo]);
    }

    #[test]
    fn identity_uses_first_non_empty_description() {
        let mut a = entry(Pacman, "x", "x");
        let mut b = entry(Flatpak, "org.x", "x");
        a.description = String::new();
        b.description = "from flatpak".into();
        let g = group(vec![a, b], &Config::default(), "x");
        assert_eq!(g[0].description, "from flatpak");
    }
}
