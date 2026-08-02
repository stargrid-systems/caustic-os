use std::ffi::OsStr;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn run(program: &str, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("exec {program}"))?;
    if !status.success() {
        bail!("{program} exited {status}");
    }
    Ok(())
}

pub fn capture_stdout(
    program: &str,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("exec {program}"))?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
