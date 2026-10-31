//! Process helpers. MPM never reimplements package operations; it shells out.

use crate::model::CommandSpec;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

/// Run a command and capture stdout. Many package managers use non-zero exit
/// codes for "no results" or "updates available", so a failure is only an
/// error when the command also produced no stdout and wrote to stderr.
pub fn capture(program: &str, args: &[&str]) -> Result<String> {
    let out = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("failed to run `{program}`"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() && stdout.trim().is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.trim();
        if !err.is_empty() {
            bail!("{}", err.lines().next().unwrap_or(err));
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
/// MPM itself survives Ctrl-C; the child gets default SIGINT behaviour.
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
