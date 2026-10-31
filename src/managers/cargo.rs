use super::{need, parse_kv, Manager};
use crate::exec;
use crate::model::*;
use anyhow::{bail, Result};

/// Crates installed via `cargo install` (user-level, no root).
pub struct Cargo;

impl Manager for Cargo {
    fn kind(&self) -> ManagerKind {
        ManagerKind::Cargo
    }
    fn binary(&self) -> String {
        "cargo".into()
    }

    fn search(&self, query: &str) -> Result<Vec<PackageEntry>> {
        let out = exec::capture("cargo", &["search", query, "--limit", "20"])?;
        let mut res = Vec::new();
        for line in out.lines() {
            // name = "1.2.3"    # description
            let Some((name, rest)) = line.split_once(" = \"") else { continue };
            let (ver, tail) = rest.split_once('"').unwrap_or((rest, ""));
            let mut e = PackageEntry::new(ManagerKind::Cargo, name.trim());
            e.version = Some(ver.to_string());
            e.description = tail.split_once('#').map(|x| x.1.trim().to_string()).unwrap_or_default();
            res.push(e);
        }
        Ok(res)
    }

    fn installed(&self) -> Result<Vec<PackageEntry>> {
        let out = exec::capture("cargo", &["install", "--list"])?;
        let mut res = Vec::new();
        for line in out.lines() {
            if line.trim().is_empty() || line.starts_with(char::is_whitespace) {
                continue; // indented lines list binaries
            }
            let mut t = line.trim_end_matches(':').split_whitespace();
            let (Some(name), Some(ver)) = (t.next(), t.next()) else { continue };
            let mut e = PackageEntry::new(ManagerKind::Cargo, name);
            e.installed_version = Some(ver.trim_start_matches('v').to_string());
            res.push(e);
        }
        Ok(res)
    }

    fn supports_updates(&self) -> bool {
        false // would need the third-party `cargo-update`; not assumed present
    }

    fn updates(&self) -> Result<Vec<UpdateInfo>> {
        Ok(vec![])
    }

    fn info(&self, e: &PackageEntry) -> Result<Vec<(String, String)>> {
        let out = exec::capture("cargo", &["info", e.id.as_str()])?;
        Ok(parse_kv(&out))
    }

    fn plan(&self, op: Op, e: Option<&PackageEntry>) -> Result<CommandSpec> {
        Ok(match op {
            Op::Install => CommandSpec::new("cargo", &["install", need(e)?.id.as_str()]),
            Op::Remove => CommandSpec::new("cargo", &["uninstall", need(e)?.id.as_str()]),
            Op::Upgrade => CommandSpec::new("cargo", &["install", need(e)?.id.as_str()])
                .note("Re-runs `cargo install`, which installs the newest version if one exists."),
            Op::UpgradeAll => bail!("cargo has no built-in upgrade-all"),
        })
    }
}
