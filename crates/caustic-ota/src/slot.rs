use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};

use crate::dd::write_image;
use crate::tar::extract;

const PARTITION_DT_PATH: &str = "/proc/device-tree/chosen/bootloader/partition";
const BOOT_EXTRACT_DIR: &str = "/var/lib/caustic-ota/boot-extract";

struct InactiveSlot {
    number: u32,
    usr_dev: &'static str,
    boot_mount: &'static str,
    cmdline_prefix: &'static str,
}

const SLOT_B: InactiveSlot = InactiveSlot {
    number: 2,
    usr_dev: "/dev/mmcblk0p6",
    boot_mount: "/boot/b",
    cmdline_prefix: "cmdline-b",
};

const SLOT_A: InactiveSlot = InactiveSlot {
    number: 1,
    usr_dev: "/dev/mmcblk0p5",
    boot_mount: "/boot/a",
    cmdline_prefix: "cmdline-a",
};

fn inactive_slot(current: u32) -> Result<&'static InactiveSlot> {
    match current {
        1 => Ok(&SLOT_B),
        2 => Ok(&SLOT_A),
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
    extract(&boot_tar, boot_dir)?;

    let cmdline_src = boot_dir.join(format!("{}.txt", slot.cmdline_prefix));

    tracing::info!(usr = %usr_image.display(), "writing usr partition");
    write_image(&usr_image, slot.usr_dev)?;

    tracing::info!(slot.boot_mount, "writing boot files");
    if !Path::new(slot.boot_mount).join("config.txt").exists() {
        bail!(
            "boot partition not mounted at {} (config.txt not found)",
            slot.boot_mount
        );
    }

    copy_boot_files(boot_dir, Path::new(slot.boot_mount), true)?;

    tracing::info!(src = %cmdline_src.display(), "writing cmdline.txt");
    fs::copy(&cmdline_src, Path::new(slot.boot_mount).join("cmdline.txt"))
        .context("copy cmdline.txt")?;

    tracing::info!(
        inactive_part = slot.number,
        "update applied to inactive slot"
    );
    Ok(())
}

fn copy_boot_files(src: &Path, dst: &Path, skip_cmdline: bool) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let path = entry?.path();
        let name = path.file_name().context("get filename")?;
        if skip_cmdline && name.to_str().is_some_and(|n| n.starts_with("cmdline-")) {
            continue;
        }
        let target = dst.join(name);
        if path.is_dir() {
            fs::create_dir_all(&target)?;
            copy_boot_files(&path, &target, false)?;
        } else {
            fs::copy(&path, &target).with_context(|| format!("copy {}", path.display()))?;
        }
    }
    Ok(())
}
