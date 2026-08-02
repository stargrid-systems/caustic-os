use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::command;

const PARTITION_DT_PATH: &str = "/proc/device-tree/chosen/bootloader/partition";
const BOOT_EXTRACT_DIR: &str = "/var/lib/caustic-ota/boot-extract";

struct InactiveSlot {
    number: u32,
    usr_dev: &'static str,
    boot_mount: &'static str,
    cmdline_prefix: &'static str,
}

fn inactive_slot(current: u32) -> Result<InactiveSlot> {
    match current {
        1 => Ok(InactiveSlot {
            number: 2,
            usr_dev: "/dev/mmcblk0p6",
            boot_mount: "/boot/b",
            cmdline_prefix: "cmdline-b",
        }),
        2 => Ok(InactiveSlot {
            number: 1,
            usr_dev: "/dev/mmcblk0p5",
            boot_mount: "/boot/a",
            cmdline_prefix: "cmdline-a",
        }),
        other => bail!("unexpected active partition: {other}"),
    }
}

fn read_current_partition() -> Result<u32> {
    let data = fs::read(PARTITION_DT_PATH).with_context(|| format!("read {PARTITION_DT_PATH}"))?;
    if data.len() >= 4 {
        Ok(u32::from_be_bytes([data[0], data[1], data[2], data[3]]))
    } else if data.len() == 1 {
        Ok(u32::from(data[0]))
    } else if let Ok(s) = std::str::from_utf8(&data) {
        s.trim()
            .parse()
            .with_context(|| format!("parse partition string: {s:?}"))
    } else {
        bail!("unexpected partition data length: {}", data.len())
    }
}

fn find_file(staging: &Path, suffix: &str) -> Result<PathBuf> {
    let mut found = None;
    for entry in fs::read_dir(staging)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            if found.is_some() {
                bail!("multiple files ending with '{suffix}' in staging dir");
            }
            found = Some(path);
        }
    }
    found.ok_or_else(|| anyhow!("no file ending with '{suffix}' in staging dir"))
}

pub fn apply_update(staging: &Path) -> Result<()> {
    let current_part = read_current_partition()?;
    let slot = inactive_slot(current_part)?;
    tracing::info!(
        current_part,
        inactive_part = slot.number,
        slot.usr_dev,
        slot.boot_mount,
        "slot info"
    );

    let usr_image = find_file(staging, ".usr")?;
    let boot_tar = find_file(staging, "_boot.tar")?;

    let boot_dir = Path::new(BOOT_EXTRACT_DIR);
    if boot_dir.exists() {
        fs::remove_dir_all(boot_dir)?;
    }
    fs::create_dir_all(boot_dir)?;
    let boot_tar_s = boot_tar.to_string_lossy();
    let boot_dir_s = boot_dir.to_string_lossy();
    command::run("tar", ["-xf", &*boot_tar_s, "-C", &*boot_dir_s])?;

    let cmdline_src = boot_dir.join(format!("{}.txt", slot.cmdline_prefix));

    tracing::info!(usr = %usr_image.display(), "writing usr partition");
    let if_arg = format!("if={}", usr_image.display());
    let of_arg = format!("of={}", slot.usr_dev);
    command::run(
        "dd",
        [if_arg.as_str(), of_arg.as_str(), "bs=128M", "conv=fsync"],
    )?;

    tracing::info!(slot.boot_mount, "writing boot files");
    if !Path::new(slot.boot_mount).is_dir() {
        bail!(
            "boot mount {} is not a directory or not mounted",
            slot.boot_mount
        );
    }

    for entry in fs::read_dir(boot_dir)? {
        let src = entry?.path();
        let name = src.file_name().context("get filename")?;
        if name.to_str().is_some_and(|n| n.starts_with("cmdline-")) {
            continue;
        }
        let dst = Path::new(slot.boot_mount).join(name);
        let src_s = src.to_string_lossy();
        let dst_s = dst.to_string_lossy();
        command::run("cp", ["-f", "-L", &*src_s, &*dst_s])
            .with_context(|| format!("copy {}", src.display()))?;
    }

    tracing::info!(src = %cmdline_src.display(), "writing cmdline.txt");
    fs::copy(&cmdline_src, Path::new(slot.boot_mount).join("cmdline.txt"))
        .context("copy cmdline.txt")?;

    tracing::info!(
        inactive_part = slot.number,
        "update applied to inactive slot"
    );
    Ok(())
}

pub fn trigger_tryboot() -> Result<()> {
    tracing::info!("triggering tryboot reboot");
    command::run("reboot", ["0 tryboot"])
}
