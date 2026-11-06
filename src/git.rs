//! `shape` delegates all version control to the standard `git` executable.

use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use anyhow::{bail, Context, Result};

pub fn run(args: &[&str]) -> Result<ExitStatus> {
    Command::new("git")
        .args(args)
        .status()
        .context("failed to run git; is it installed and on PATH?")
}

pub fn run_ok(args: &[&str]) -> Result<()> {
    let status = run(args)?;
    if !status.success() {
        bail!("git {} failed", args.first().copied().unwrap_or(""));
    }
    Ok(())
}

pub fn output(args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .context("failed to run git")?;
    if !out.status.success() {
        bail!("git {} failed", args.first().copied().unwrap_or(""));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

pub fn in_repo() -> bool {
    output(&["rev-parse", "--is-inside-work-tree"])
        .map(|s| s == "true")
        .unwrap_or(false)
}

pub fn lfs_available() -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Bytes of a file at a revision (LFS pointers are smudged through `git lfs`).
pub fn show_bytes(rev: &str, path: &str) -> Result<Vec<u8>> {
    let out = Command::new("git")
        .args(["show", &format!("{rev}:{path}")])
        .stderr(Stdio::null())
        .output()?;
    if !out.status.success() {
        bail!("{path} does not exist at {rev}");
    }
    let data = out.stdout;
    if data.starts_with(b"version https://git-lfs.github.com/spec/v1") && lfs_available() {
        let mut child = Command::new("git")
            .args(["lfs", "smudge", "--", path])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;
        use std::io::Write;
        child.stdin.take().unwrap().write_all(&data)?;
        let out = child.wait_with_output()?;
        if out.status.success() {
            return Ok(out.stdout);
        }
    }
    Ok(data)
}

pub fn repo_root() -> Result<std::path::PathBuf> {
    Ok(Path::new(&output(&["rev-parse", "--show-toplevel"])?).to_path_buf())
}
