use super::{need, Manager};
use crate::exec;
use crate::model::*;
use anyhow::{Context, Result};
use serde_json::Value;

/// Global npm packages only (`-g`). Local project dependencies are out of scope.
pub struct Npm;

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

impl Manager for Npm {
    fn kind(&self) -> ManagerKind {
        ManagerKind::Npm
    }
    fn binary(&self) -> String {
        "npm".into()
    }

    fn search(&self, query: &str) -> Result<Vec<PackageEntry>> {
        let out = exec::capture("npm", &["search", "--json", "--searchlimit=25", query])?;
        if out.trim().is_empty() {
            return Ok(vec![]);
        }
        let v: Value = serde_json::from_str(&out).context("unexpected `npm search` output")?;
        Ok(v.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        let mut e = PackageEntry::new(ManagerKind::Npm, s(p, "name")?);
                        e.description = s(p, "description").unwrap_or_default();
                        e.version = s(p, "version");
                        Some(e)
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn installed(&self) -> Result<Vec<PackageEntry>> {
        let out = exec::capture("npm", &["ls", "-g", "--depth=0", "--json"])?;
        if out.trim().is_empty() {
            return Ok(vec![]);
        }
        let v: Value = serde_json::from_str(&out).context("unexpected `npm ls` output")?;
        let mut res = Vec::new();
        if let Some(deps) = v.get("dependencies").and_then(Value::as_object) {
            for (name, info) in deps {
                let mut e = PackageEntry::new(ManagerKind::Npm, name.clone());
                e.installed_version = Some(s(info, "version").unwrap_or_else(|| "?".into()));
                res.push(e);
            }
        }
        Ok(res)
    }

    fn updates(&self) -> Result<Vec<UpdateInfo>> {
        // `npm outdated` exits 1 when something is outdated; capture() tolerates that.
        let out = exec::capture("npm", &["outdated", "-g", "--json"])?;
        if out.trim().is_empty() {
            return Ok(vec![]);
        }
        let v: Value = serde_json::from_str(&out).context("unexpected `npm outdated` output")?;
        Ok(v.as_object()
            .map(|m| {
                m.iter()
                    .map(|(name, i)| UpdateInfo {
                        id: name.clone(),
                        from: s(i, "current").unwrap_or_else(|| "?".into()),
                        to: s(i, "latest").unwrap_or_else(|| "?".into()),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    fn info(&self, e: &PackageEntry) -> Result<Vec<(String, String)>> {
        let out = exec::capture("npm", &["view", e.id.as_str(), "--json"])?;
        let v: Value = serde_json::from_str(&out).context("unexpected `npm view` output")?;
        let mut res = Vec::new();
        for (label, key) in [
            ("Version", "version"),
            ("Description", "description"),
            ("Homepage", "homepage"),
            ("License", "license"),
        ] {
            if let Some(x) = s(&v, key) {
                res.push((label.to_string(), x));
            }
        }
        if let Some(m) = v.get("maintainers").and_then(Value::as_array) {
            let names: Vec<&str> = m.iter().filter_map(Value::as_str).collect();
            if !names.is_empty() {
                res.push(("Maintainers".into(), names.join(", ")));
            }
        }
        if let Some(d) = v.get("dependencies").and_then(Value::as_object) {
            let names: Vec<&str> = d.keys().map(String::as_str).collect();
            res.push((
                "Dependencies".into(),
                if names.is_empty() { "none".into() } else { names.join(", ") },
            ));
        }
        Ok(res)
    }

    fn plan(&self, op: Op, e: Option<&PackageEntry>) -> Result<CommandSpec> {
        // No escalation: npm's global prefix decides what's writable (spec 15).
        Ok(match op {
            Op::Install => CommandSpec::new("npm", &["install", "-g", need(e)?.id.as_str()]),
            Op::Remove => CommandSpec::new("npm", &["uninstall", "-g", need(e)?.id.as_str()]),
            Op::Upgrade => CommandSpec::new("npm", &["update", "-g", need(e)?.id.as_str()]),
            Op::UpgradeAll => CommandSpec::new("npm", &["update", "-g"]),
        })
    }
}
