use super::{need, pacman, parse_updates, Manager};
use crate::exec;
use crate::model::*;
use anyhow::{bail, Result};

/// AUR access through a helper (yay / paru). Installed AUR packages live in
/// the pacman database ("foreign" packages), so removal goes through pacman.
pub struct Aur {
    pub helper: String,
}

impl Manager for Aur {
    fn kind(&self) -> ManagerKind {
        ManagerKind::Aur
    }
    fn binary(&self) -> String {
        self.helper.clone()
    }

    fn probe(&self) -> Result<()> {
        if exec::which("pacman").is_none() {
            bail!("pacman not found (required by AUR helpers)");
        }
        Ok(())
    }

    fn search(&self, query: &str) -> Result<Vec<PackageEntry>> {
        let out = exec::capture(&self.helper, &["-Ssa", query])?;
        Ok(pacman::parse_ss(&out, ManagerKind::Aur, Some("aur")))
    }

    fn installed(&self) -> Result<Vec<PackageEntry>> {
        let (entries, foreign) = pacman::query_installed()?;
        Ok(entries
            .into_iter()
            .filter(|e| foreign.contains(&e.id))
            .map(|mut e| {
                e.manager = ManagerKind::Aur;
                e.repo = Some("aur".into());
                e
            })
            .collect())
    }

    fn updates(&self) -> Result<Vec<UpdateInfo>> {
        let out = exec::capture(&self.helper, &["-Qua"])?;
        Ok(parse_updates(&out))
    }

    fn info(&self, e: &PackageEntry) -> Result<Vec<(String, String)>> {
        let out = if e.is_installed() {
            exec::capture("pacman", &["-Qi", e.id.as_str()])?
        } else {
            exec::capture(&self.helper, &["-Si", e.id.as_str()])?
        };
        Ok(pacman::parse_info(&out).into_iter().next().unwrap_or_default())
    }

    fn plan(&self, op: Op, e: Option<&PackageEntry>) -> Result<CommandSpec> {
        Ok(match op {
            // Helpers call sudo themselves; Zito does not escalate for them.
            Op::Install | Op::Upgrade => {
                CommandSpec::new(&self.helper, &["-S", need(e)?.id.as_str()])
            }
            Op::Remove => CommandSpec::new("pacman", &["-Rs", need(e)?.id.as_str()]).root(),
            Op::UpgradeAll => CommandSpec::new(&self.helper, &["-Sua"]),
        })
    }
}
