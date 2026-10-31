use super::{need, parse_updates, Manager};
use crate::exec;
use crate::model::*;
use anyhow::Result;
use std::collections::HashSet;

pub struct Pacman;

/// Parses `pacman -Ss` / `yay -Ssa` output: a header line followed by an
/// indented description.
pub(crate) fn parse_ss(out: &str, kind: ManagerKind, force_repo: Option<&str>) -> Vec<PackageEntry> {
    let mut v: Vec<PackageEntry> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(last) = v.last_mut() {
                if !last.description.is_empty() {
                    last.description.push(' ');
                }
                last.description.push_str(line.trim());
            }
            continue;
        }
        let mut t = line.split_whitespace();
        let (Some(first), Some(ver)) = (t.next(), t.next()) else { continue };
        let (repo, name) = first.split_once('/').unwrap_or(("", first));
        let repo = force_repo.unwrap_or(repo);
        let mut e = PackageEntry::new(kind, name);
        e.version = Some(ver.to_string());
        e.repo = (!repo.is_empty()).then(|| repo.to_string());
        v.push(e);
    }
    v
}

/// Parses `pacman -Qi` style blocks: "Key : Value", blank line between
/// packages, indented continuation lines.
pub(crate) fn parse_info(out: &str) -> Vec<Vec<(String, String)>> {
    let mut blocks = Vec::new();
    let mut cur: Vec<(String, String)> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                blocks.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if line.starts_with(char::is_whitespace) {
            if let Some(last) = cur.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((k, v)) = line.split_once(':') {
            cur.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    if !cur.is_empty() {
        blocks.push(cur);
    }
    blocks
}

/// All installed pacman-database packages, plus the set of "foreign" names
/// (not in any sync repo = AUR / manual). Shared with the AUR manager.
pub(crate) fn query_installed() -> Result<(Vec<PackageEntry>, HashSet<String>)> {
    let out = exec::capture("pacman", &["-Qi"])?;
    let foreign: HashSet<String> = exec::capture("pacman", &["-Qmq"])?
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let mut entries = Vec::new();
    for block in parse_info(&out) {
        let get = |k: &str| block.iter().find(|(kk, _)| kk == k).map(|(_, v)| v.clone());
        let Some(name) = get("Name") else { continue };
        let mut e = PackageEntry::new(ManagerKind::Pacman, name);
        e.installed_version = get("Version");
        e.description = get("Description").unwrap_or_default();
        e.explicit = get("Install Reason").map(|r| r.contains("Explicitly"));
        entries.push(e);
    }
    Ok((entries, foreign))
}

impl Manager for Pacman {
    fn kind(&self) -> ManagerKind {
        ManagerKind::Pacman
    }
    fn binary(&self) -> String {
        "pacman".into()
    }

    fn version(&self) -> Option<String> {
        exec::capture("pacman", &["--version"])
            .ok()?
            .lines()
            .find(|l| l.contains("Pacman v"))
            .map(|l| l.trim().to_string())
    }

    fn search(&self, query: &str) -> Result<Vec<PackageEntry>> {
        let out = exec::capture("pacman", &["-Ss", query])?;
        Ok(parse_ss(&out, ManagerKind::Pacman, None))
    }

    fn installed(&self) -> Result<Vec<PackageEntry>> {
        let (entries, foreign) = query_installed()?;
        Ok(entries.into_iter().filter(|e| !foreign.contains(&e.id)).collect())
    }

    fn updates(&self) -> Result<Vec<UpdateInfo>> {
        // checkupdates (pacman-contrib) uses a temp db and never touches the
        // system db; fall back to `pacman -Qu` (reflects last sync only).
        let out = if exec::which("checkupdates").is_some() {
            exec::capture("checkupdates", &[])?
        } else {
            exec::capture("pacman", &["-Qu"])?
        };
        Ok(parse_updates(&out))
    }

    fn orphans(&self) -> Result<Vec<String>> {
        let out = exec::capture("pacman", &["-Qdtq"])?;
        Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
    }

    fn info(&self, e: &PackageEntry) -> Result<Vec<(String, String)>> {
        let out = if e.is_installed() {
            exec::capture("pacman", &["-Qi", e.id.as_str()])?
        } else {
            let target = match &e.repo {
                Some(r) => format!("{r}/{}", e.id),
                None => e.id.clone(),
            };
            exec::capture("pacman", &["-Si", target.as_str()])?
        };
        Ok(parse_info(&out).into_iter().next().unwrap_or_default())
    }

    fn plan(&self, op: Op, e: Option<&PackageEntry>) -> Result<CommandSpec> {
        Ok(match op {
            Op::Install => {
                let e = need(e)?;
                let target = match &e.repo {
                    Some(r) => format!("{r}/{}", e.id),
                    None => e.id.clone(),
                };
                CommandSpec::new("pacman", &["-S", "--needed", target.as_str()]).root()
            }
            Op::Remove => {
                let e = need(e)?;
                CommandSpec::new("pacman", &["-Rs", e.id.as_str()]).root()
            }
            Op::Upgrade => CommandSpec::new("pacman", &["-Syu"]).root().note(
                "Arch does not support partial upgrades: upgrading one package runs a full system upgrade.",
            ),
            Op::UpgradeAll => CommandSpec::new("pacman", &["-Syu"]).root(),
        })
    }
}
