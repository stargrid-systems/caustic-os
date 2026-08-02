use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

pub fn write_image(image: &Path, device: &str) -> Result<()> {
    let image_size = fs::metadata(image)
        .with_context(|| format!("stat image {}", image.display()))?
        .len();

    let dev_size = device_size(device)?;

    if image_size > dev_size {
        bail!("image ({image_size} bytes) exceeds {device} capacity ({dev_size} bytes)");
    }

    let if_arg = format!("if={}", image.display());
    let of_arg = format!("of={device}");

    let status = Command::new("dd")
        .arg(&if_arg)
        .arg(&of_arg)
        .arg("bs=128M")
        .arg("conv=fsync")
        .status()
        .with_context(|| format!("exec dd to {device}"))?;

    if !status.success() {
        bail!("dd exited {status}");
    }

    Ok(())
}

fn device_size(device: &str) -> Result<u64> {
    let output = Command::new("blockdev")
        .arg("--getsize64")
        .arg(device)
        .output()
        .with_context(|| format!("exec blockdev for {device}"))?;

    if !output.status.success() {
        bail!("blockdev exited {}", output.status);
    }

    let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
    s.parse::<u64>().with_context(|| format!("parse size: {s}"))
}
