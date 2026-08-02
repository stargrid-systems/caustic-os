use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow, bail};
use caustic_oci::{self, OciImageManifest};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

const STAGING_DIR: &str = "/var/lib/caustic-ota/staging";
const STATE_FILE: &str = "/var/lib/caustic-ota/state.json";
const SYSTEMCTL_BIN: &str = "/run/current-system/sw/bin/systemctl";
const FACTORY_RESET_SENTINEL: &str = "/persist/.factory-reset";

const PARTITION_DT_PATH: &str = "/proc/device-tree/chosen/bootloader/partition";

#[derive(Parser)]
#[command(name = "caustic-ota", version, about = "Caustic OS OTA update daemon")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Check {
        #[arg(long, default_value = "ghcr.io/stargrid-systems/caustic-os")]
        registry: String,
        #[arg(long, default_value = "latest")]
        tag: String,
    },
    Update {
        #[arg(long, default_value = "ghcr.io/stargrid-systems/caustic-os")]
        registry: String,
        #[arg(long, default_value = "latest")]
        tag: String,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    FactoryReset,
}

#[derive(Serialize, Deserialize)]
struct State {
    current_version: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("info".parse()?),
        )
        .init();

    let cli = Cli::parse();
    let token = std::env::var("CAUSTIC_OTA_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    match cli.command {
        Commands::Check { registry, tag } => check(&registry, &tag, token.as_deref()).await,
        Commands::Update {
            registry,
            tag,
            force,
        } => update(&registry, &tag, force, token.as_deref()).await,
        Commands::FactoryReset => factory_reset(),
    }
}

async fn check(registry: &str, tag: &str, token: Option<&str>) -> Result<()> {
    let manifest = caustic_oci::fetch_manifest(registry, tag, token)
        .await
        .with_context(|| format!("pull manifest from {registry}:{tag}"))?;
    let version = caustic_oci::extract_version(&manifest)?;
    let state = read_state().unwrap_or(State {
        current_version: String::new(),
    });
    if version == state.current_version {
        tracing::info!(%version, "already up to date");
        println!("up-to-date");
    } else {
        tracing::info!(%version, current = %state.current_version, "update available");
        println!("update-available {version}");
    }
    Ok(())
}

async fn update(registry: &str, tag: &str, force: bool, token: Option<&str>) -> Result<()> {
    if !force {
        verify_boot_healthy()?;
    }

    let manifest = caustic_oci::fetch_manifest(registry, tag, token)
        .await
        .with_context(|| format!("pull manifest from {registry}:{tag}"))?;
    let version = caustic_oci::extract_version(&manifest)?;
    let state = read_state().unwrap_or(State {
        current_version: String::new(),
    });
    if version == state.current_version {
        tracing::info!(%version, "already up to date");
        return Ok(());
    }

    tracing::info!(%version, "preparing update");
    let staging = Path::new(STAGING_DIR);
    fs::create_dir_all(staging).context("create staging dir")?;
    clear_dir(staging)?;

    pull_layers(registry, tag, &manifest, staging, token).await?;
    caustic_oci::verify_sha256sums(staging).context("verify checksums")?;

    tracing::info!("applying update to inactive slot");
    apply_update(staging).context("apply update")?;

    write_state(&State {
        current_version: version.clone(),
    })?;
    tracing::info!(%version, "update staged, tryboot reboot pending");
    trigger_tryboot()?;
    Ok(())
}

fn verify_boot_healthy() -> Result<()> {
    let output = Command::new(SYSTEMCTL_BIN)
        .args(["is-system-running"])
        .output()
        .context("run systemctl is-system-running")?;
    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match status.as_str() {
        "running" | "degraded" => Ok(()),
        other => Err(anyhow!(
            "current boot is unhealthy ({other}); refusing to update (use --force to override)"
        )),
    }
}

fn read_current_partition() -> Result<u32> {
    let data = fs::read(PARTITION_DT_PATH)
        .with_context(|| format!("read {PARTITION_DT_PATH}"))?;
    if data.len() >= 4 {
        Ok(u32::from_be_bytes([
            data[0], data[1], data[2], data[3],
        ]))
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
    let mut found = Vec::new();
    for entry in fs::read_dir(staging)? {
        let path = entry?.path();
        if path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|name| name.ends_with(suffix))
        {
            found.push(path);
        }
    }
    match found.len() {
        1 => Ok(found.into_iter().next().unwrap()),
        0 => bail!("no file ending with '{suffix}' in staging dir"),
        _ => bail!("multiple files ending with '{suffix}' in staging dir"),
    }
}

fn apply_update(staging: &Path) -> Result<()> {
    let current_part = read_current_partition()?;
    let (inactive_part, usr_dev, boot_mount, cmdline_prefix) = match current_part {
        1 => (2u32, "/dev/mmcblk0p6", "/boot/b", "cmdline-b"),
        2 => (1, "/dev/mmcblk0p5", "/boot/a", "cmdline-a"),
        other => bail!("unexpected active partition: {other}"),
    };
    tracing::info!(current_part, inactive_part, usr_dev, boot_mount, "slot info");

    let usr_image = find_file(staging, ".usr")?;
    let boot_tar = find_file(staging, "_boot.tar")?;

    let boot_dir = Path::new("/var/lib/caustic-ota/boot-extract");
    if boot_dir.exists() {
        fs::remove_dir_all(boot_dir)?;
    }
    fs::create_dir_all(boot_dir)?;
    let status = Command::new("tar")
        .args(["-xf"])
        .arg(&boot_tar)
        .args(["-C"])
        .arg(boot_dir)
        .status()
        .context("extract boot tar")?;
    if !status.success() {
        bail!("tar extract failed with status {status:?}");
    }

    let cmdline_src = boot_dir.join(format!("{cmdline_prefix}.txt"));

    tracing::info!(usr = %usr_image.display(), "writing usr partition");
    let status = Command::new("dd")
        .args([
            "if=".to_string() + &usr_image.to_string_lossy(),
            "of=".to_string() + usr_dev,
            "bs=128M".to_string(),
            "conv=fsync".to_string(),
        ])
        .status()
        .context("run dd")?;
    if !status.success() {
        bail!("dd failed with status {status:?}");
    }

    tracing::info!(boot_mount, "writing boot files");
    if !Path::new(boot_mount).is_dir() {
        bail!("boot mount {boot_mount} is not a directory or not mounted");
    }

    let entries = fs::read_dir(boot_dir).context("read boot dir")?;
    for entry in entries {
        let src = entry?.path();
        let name = src.file_name().context("get filename")?;
        if name.to_str().is_some_and(|n| n.starts_with("cmdline-")) {
            continue;
        }
        let dst = Path::new(boot_mount).join(name);
        let status = Command::new("cp")
            .args(["-f", "-L"])
            .arg(&src)
            .arg(&dst)
            .status()
            .with_context(|| format!("copy {}", src.display()))?;
        if !status.success() {
            bail!("cp {} failed", src.display());
        }
    }

    tracing::info!(src = %cmdline_src.display(), "writing cmdline.txt");
    fs::copy(&cmdline_src, Path::new(boot_mount).join("cmdline.txt"))
        .context("copy cmdline.txt")?;

    tracing::info!(inactive_part, "update applied to inactive slot");
    Ok(())
}

fn trigger_tryboot() -> Result<()> {
    tracing::info!("triggering tryboot reboot");
    let status = Command::new("reboot")
        .arg("0 tryboot")
        .status()
        .context("run reboot tryboot")?;
    if !status.success() {
        bail!("reboot tryboot failed with status {status:?}");
    }
    Ok(())
}

fn factory_reset() -> Result<()> {
    let sentinel = Path::new(FACTORY_RESET_SENTINEL);
    if let Some(parent) = sentinel.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(sentinel, "1\n").context("write factory-reset sentinel")?;
    tracing::info!("factory reset requested. Reboot to complete.");
    Ok(())
}

async fn pull_layers(
    registry: &str,
    tag: &str,
    manifest: &OciImageManifest,
    staging: &Path,
    token: Option<&str>,
) -> Result<()> {
    for layer in &manifest.layers {
        let name = layer
            .annotations
            .as_ref()
            .and_then(|annotations| annotations.get("org.opencontainers.image.title"))
            .ok_or_else(|| anyhow!("layer missing title annotation"))?;

        if name.contains('/') || name.contains("..") || name.contains(std::path::MAIN_SEPARATOR) {
            return Err(anyhow!(
                "layer title contains unsafe path characters: {name}"
            ));
        }

        tracing::info!(%name, digest = %layer.digest, "pulling layer");
        let dst = staging.join(name);
        caustic_oci::pull_blob(registry, tag, layer, &dst, token)
            .await
            .with_context(|| format!("pull layer {name}"))?;
        tracing::info!(%name, "pulled");
    }
    Ok(())
}

fn clear_dir(dir: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn read_state() -> Result<State> {
    let bytes = fs::read(STATE_FILE).context("read state")?;
    serde_json::from_slice(&bytes).context("parse state")
}

fn write_state(state: &State) -> Result<()> {
    let path = PathBuf::from(STATE_FILE);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    let bytes = serde_json::to_vec_pretty(state)?;
    fs::write(path, bytes).context("write state")
}
