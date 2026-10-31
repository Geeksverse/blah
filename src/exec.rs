//! Process helpers. Zito never reimplements package operations; it shells out.

use crate::model::CommandSpec;
use anyhow::{anyhow, bail, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Coarse failure category, derived from a provider's stderr. The original
/// first stderr line is always kept; this only adds a readable prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    Permission,
    Network,
    NotFound,
    Other,
}

impl FailureKind {
    fn prefix(self) -> &'static str {
        match self {
            FailureKind::Permission => "permission: ",
            FailureKind::Network => "network: ",
            FailureKind::NotFound => "not found: ",
            FailureKind::Other => "",
        }
    }
}

pub fn classify(stderr: &str) -> FailureKind {
    let s = stderr.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|n| s.contains(n));
    if has(&["permission denied", "operation not permitted", "eacces", "must be root", "unless you are root"]) {
        FailureKind::Permission
    } else if has(&[
        "could not resolve",
        "failed retrieving file",
        "timed out",
        "connection refused",
        "unable to connect",
        "network",
        "enotfound",
        "etimedout",
        "econnrefused",
    ]) {
        FailureKind::Network
    } else if has(&["target not found", "no matching package", "package not found"]) {
        FailureKind::NotFound
    } else {
        FailureKind::Other
    }
}

/// Run a command and capture stdout. Many package managers use non-zero exit
/// codes for "no results" or "updates available", so a failure is only an
/// error when the command also produced no stdout and wrote to stderr.
pub fn capture(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => anyhow!("executable `{program}` not found in PATH"),
            _ => anyhow::Error::new(e).context(format!("failed to run `{program}`")),
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() && stdout.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        if !err.is_empty() {
            bail!("{}{}", classify(err).prefix(), err.lines().next().unwrap_or(err));
        }
    }
    Ok(stdout)
}

pub fn which(bin: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(bin))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata()
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

pub fn is_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid() == 0).unwrap_or(false)
}

/// Run a command attached to the real terminal (so sudo / manager prompts work).
/// Zito itself survives Ctrl-C; the child gets default SIGINT behaviour.
pub fn run_interactive(spec: &CommandSpec) -> std::io::Result<ExitStatus> {
    extern "C" fn noop(_: libc::c_int) {}
    let argv = spec.argv(is_root());
    let (prog, rest) = argv.split_first().expect("argv is never empty");
    unsafe {
        // A *handler* (not SIG_IGN) is reset to default across exec().
        libc::signal(libc::SIGINT, noop as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
    let r = Command::new(prog).args(rest).status();
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognises_common_failures() {
        assert_eq!(classify("error: Permission denied"), FailureKind::Permission);
        assert_eq!(classify("npm ERR! code ENOTFOUND"), FailureKind::Network);
        assert_eq!(classify("error: failed retrieving file 'x' from mirror"), FailureKind::Network);
        assert_eq!(classify("error: target not found: nope"), FailureKind::NotFound);
        assert_eq!(classify("something odd happened"), FailureKind::Other);
    }

    #[test]
    fn missing_executable_is_reported_distinctly() {
        let e = capture("zito-definitely-not-a-real-binary", &[]).unwrap_err();
        assert!(e.to_string().contains("not found in PATH"), "{e}");
    }

    #[test]
    fn nonzero_exit_with_stdout_is_not_an_error() {
        // `sh -c 'echo hit; exit 1'` mimics "results found, exit 1" managers.
        let out = capture("sh", &["-c", "echo hit; exit 1"]).unwrap();
        assert_eq!(out.trim(), "hit");
    }

    #[test]
    fn failure_prefix_and_first_stderr_line_are_kept() {
        let e = capture("sh", &["-c", "echo 'Permission denied: x' >&2; exit 1"]).unwrap_err();
        assert_eq!(e.to_string(), "permission: Permission denied: x");
    }
}
