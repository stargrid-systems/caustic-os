use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn extract(archive: &Path, dest: &Path) -> Result<()> {
    let archive_s = archive.to_string_lossy();
    let dest_s = dest.to_string_lossy();

    let status = Command::new("tar")
        .arg("-xf")
        .arg(&*archive_s)
        .arg("-C")
        .arg(&*dest_s)
        .status()
        .context("exec tar")?;

    if !status.success() {
        bail!("tar exited {status}");
    }

    Ok(())
}
