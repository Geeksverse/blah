//! Software identity grouping.
//!
//! Deliberately conservative: wrongly merging unrelated packages is worse than
//! showing them separately (spec 4.2). Current rule: entries from application
//! sources (pacman / aur / flatpak) are merged only when their normalized
//! display names are exactly equal. npm and cargo entries are never merged.
//! The real matching algorithm is an OPEN requirement (spec 21).

use crate::config::Config;
use crate::model::{PackageEntry, SoftwareIdentity};
use std::collections::BTreeMap;

pub fn normalize(s: &str) -> String {
    s.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-")
}

pub fn group(entries: Vec<PackageEntry>, cfg: &Config, query: &str) -> Vec<SoftwareIdentity> {
    let mut map: BTreeMap<String, Vec<PackageEntry>> = BTreeMap::new();
    for e in entries {
        let key = if e.manager.groupable() {
            format!("app:{}", normalize(&e.name))
        } else {
            format!("{}:{}", e.manager.label(), e.id)
        };
        map.entry(key).or_default().push(e);
    }
    let q = normalize(query);
    let mut out: Vec<SoftwareIdentity> = map
        .into_values()
        .map(|mut es| {
            es.sort_by_key(|e| (cfg.rank(e.manager), e.id.clone()));
            identity_from(es)
        })
        .collect();
    out.sort_by_key(|i| {
        let n = normalize(&i.name);
        let rank = if n == q {
            0
        } else if n.starts_with(&q) {
            1
        } else if n.contains(&q) {
            2
        } else {
            3
        };
        (rank, n)
    });
    out
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
    SoftwareIdentity { name, description, entries: es }
}
