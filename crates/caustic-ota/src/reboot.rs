use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn tryboot() -> Result<()> {
    let status = Command::new("reboot")
        .arg("0 tryboot")
        .status()
        .context("exec reboot")?;

    if !status.success() {
        bail!("reboot exited {status}");
    }

    Ok(())
}
