use super::{need, ne, parse_kv, Manager};
use crate::exec;
use crate::model::*;
use anyhow::{bail, Result};

pub struct Flatpak;

/// Tab-separated rows; header rows and noise are dropped because an app id
/// always contains a dot.
fn rows(out: &str, min_cols: usize) -> Vec<Vec<String>> {
    out.lines()
        .map(|l| l.split('\t').map(|c| c.trim().to_string()).collect::<Vec<_>>())
        .filter(|c| c.len() >= min_cols && c[0].contains('.'))
        .collect()
}

impl Manager for Flatpak {
    fn kind(&self) -> ManagerKind {
        ManagerKind::Flatpak
    }
    fn binary(&self) -> String {
        "flatpak".into()
    }

    fn probe(&self) -> Result<()> {
        let o = exec::capture("flatpak", &["remotes"])?;
        if o.trim().is_empty() {
            bail!("no flatpak remotes configured");
        }
        Ok(())
    }

    fn search(&self, query: &str) -> Result<Vec<PackageEntry>> {
        let out = exec::capture(
            "flatpak",
            &["search", "--columns=application,name,description,version,remotes", query],
        )?;
        Ok(rows(&out, 5)
            .into_iter()
            .map(|c| {
                let mut e = PackageEntry::new(ManagerKind::Flatpak, c[0].clone());
                e.name = c[1].clone();
                e.description = c[2].clone();
                e.version = ne(&c[3]);
                e.repo = c[4].split(',').next().and_then(ne);
                e
            })
            .collect())
    }

    fn installed(&self) -> Result<Vec<PackageEntry>> {
        let out = exec::capture(
            "flatpak",
            &["list", "--app", "--columns=application,name,version,description,origin"],
        )?;
        Ok(rows(&out, 5)
            .into_iter()
            .map(|c| {
                let mut e = PackageEntry::new(ManagerKind::Flatpak, c[0].clone());
                e.name = c[1].clone();
                e.installed_version = Some(ne(&c[2]).unwrap_or_else(|| "-".to_string()));
                e.description = c[3].clone();
                e.repo = ne(&c[4]);
                e
            })
            .collect())
    }

    fn updates(&self) -> Result<Vec<UpdateInfo>> {
        let out = exec::capture(
            "flatpak",
            &["remote-ls", "--updates", "--app", "--columns=application,version"],
        )?;
        Ok(rows(&out, 2)
            .into_iter()
            .map(|c| UpdateInfo {
                id: c[0].clone(),
                from: "?".into(),
                to: ne(&c[1]).unwrap_or_else(|| "?".into()),
            })
            .collect())
    }

    fn info(&self, e: &PackageEntry) -> Result<Vec<(String, String)>> {
        let out = if e.is_installed() {
            exec::capture("flatpak", &["info", e.id.as_str()])?
        } else {
            let remote = e.repo.clone().unwrap_or_else(|| "flathub".into());
            exec::capture("flatpak", &["remote-info", remote.as_str(), e.id.as_str()])?
        };
        Ok(parse_kv(&out))
    }

    fn plan(&self, op: Op, e: Option<&PackageEntry>) -> Result<CommandSpec> {
        // Install scope (--user / --system) is left to flatpak's own defaults.
        Ok(match op {
            Op::Install => {
                let e = need(e)?;
                match &e.repo {
                    Some(r) => CommandSpec::new("flatpak", &["install", r.as_str(), e.id.as_str()]),
                    None => CommandSpec::new("flatpak", &["install", e.id.as_str()]),
                }
            }
            Op::Remove => CommandSpec::new("flatpak", &["uninstall", need(e)?.id.as_str()]),
            Op::Upgrade => CommandSpec::new("flatpak", &["update", need(e)?.id.as_str()]),
            Op::UpgradeAll => CommandSpec::new("flatpak", &["update"]),
        })
    }
}
