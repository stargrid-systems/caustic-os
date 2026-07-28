use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, anyhow};
use caustic_oci::{self, OciImageManifest};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

const STAGING_DIR: &str = "/var/lib/caustic-ota/staging";
const STATE_FILE: &str = "/var/lib/caustic-ota/state.json";
const SYSUPDATE_BIN: &str = "/run/current-system/sw/bin/systemd-sysupdate";
const SYSTEMCTL_BIN: &str = "/run/current-system/sw/bin/systemctl";
const FACTORY_RESET_SENTINEL: &str = "/persist/.factory-reset";

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

    tracing::info!("invoking systemd-sysupdate");
    let status = Command::new(SYSUPDATE_BIN)
        .arg("update")
        .status()
        .context("run systemd-sysupdate")?;
    if !status.success() {
        return Err(anyhow!("systemd-sysupdate failed with status {status:?}"));
    }

    write_state(&State {
        current_version: version.clone(),
    })?;
    tracing::info!(%version, "update staged, reboot pending");
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

fn factory_reset() -> Result<()> {
    let sentinel = Path::new(FACTORY_RESET_SENTINEL);
    if let Some(parent) = sentinel.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(sentinel, "1\n").context("write factory-reset sentinel")?;
    tracing::info!("factory reset requested; reboot to complete");
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
