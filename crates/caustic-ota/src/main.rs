use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use oci_distribution::{
    client::ClientConfig,
    manifest::OciImageManifest,
    secrets::RegistryAuth,
    Client, Reference,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tokio::io::AsyncWriteExt;
use tracing::{info, warn};

const STAGING_DIR: &str = "/var/lib/caustic-ota/staging";
const STATE_FILE: &str = "/var/lib/caustic-ota/state.json";
const SYSUPDATE_BIN: &str = "/run/current-system/sw/bin/systemd-sysupdate";
const SYSTEMCTL_BIN: &str = "/run/current-system/sw/bin/systemctl";
const FACTORY_RESET_SENTINEL: &str = "/persist/.factory-reset";
const VERSION_ANNOTATION: &str = "org.opencontainers.image.version";

#[derive(Parser)]
#[command(name = "caustic-ota", version, about = "Caustic OS OTA update daemon")]
struct Cli {
    #[command(subcommand)]
    command: Command_,
}

#[derive(Subcommand)]
enum Command_ {
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
    match cli.command {
        Command_::Check { registry, tag } => check(&registry, &tag).await,
        Command_::Update {
            registry,
            tag,
            force,
        } => update(&registry, &tag, force).await,
        Command_::FactoryReset => factory_reset(),
    }
}

async fn check(registry: &str, tag: &str) -> Result<()> {
    let manifest = fetch_manifest(registry, tag).await?;
    let version = extract_version(&manifest)?;
    let state = read_state().unwrap_or_else(|_| State { current_version: String::new() });
    if version == state.current_version {
        info!(%version, "already up to date");
        println!("up-to-date");
    } else {
        info!(%version, current = %state.current_version, "update available");
        println!("update-available {version}");
    }
    Ok(())
}

async fn update(registry: &str, tag: &str, force: bool) -> Result<()> {
    if !force {
        verify_boot_healthy()?;
    }

    let manifest = fetch_manifest(registry, tag).await?;
    let version = extract_version(&manifest)?;
    let state = read_state().unwrap_or_else(|_| State { current_version: String::new() });
    if version == state.current_version {
        info!(%version, "already up to date");
        return Ok(());
    }

    info!(%version, "preparing update");
    let staging = Path::new(STAGING_DIR);
    fs::create_dir_all(staging).context("create staging dir")?;
    clear_dir(staging)?;

    pull_layers(registry, tag, &manifest, staging).await?;
    verify_sha256sums(staging)?;

    info!("invoking systemd-sysupdate");
    let status = Command::new(SYSUPDATE_BIN)
        .arg("update")
        .status()
        .context("run systemd-sysupdate")?;
    if !status.success() {
        return Err(anyhow!("systemd-sysupdate failed with status {status:?}"));
    }

    write_state(&State { current_version: version.clone() })?;
    info!(%version, "update staged, reboot pending");
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
        fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))?;
    }
    fs::write(sentinel, "1\n").context("write factory-reset sentinel")?;
    info!("factory reset requested; reboot to complete");
    Ok(())
}

async fn fetch_manifest(registry: &str, tag: &str) -> Result<OciImageManifest> {
    let reference: Reference = format!("{registry}:{tag}").parse()?;
    let client = Client::new(ClientConfig::default());
    let auth = RegistryAuth::Anonymous;
    let (manifest, _) = client
        .pull_image_manifest(&reference, &auth)
        .await
        .with_context(|| format!("pull manifest from {registry}:{tag}"))?;
    Ok(manifest)
}

fn extract_version(manifest: &OciImageManifest) -> Result<String> {
    manifest
        .annotations
        .as_ref()
        .and_then(|a| a.get(VERSION_ANNOTATION))
        .cloned()
        .ok_or_else(|| anyhow!("manifest missing {VERSION_ANNOTATION} annotation"))
}

async fn pull_layers(
    registry: &str,
    tag: &str,
    manifest: &OciImageManifest,
    staging: &Path,
) -> Result<()> {
    let reference: Reference = format!("{registry}:{tag}").parse()?;
    let client = Client::new(ClientConfig::default());

    for layer in &manifest.layers {
        let name = layer
            .annotations
            .as_ref()
            .and_then(|a| a.get("org.opencontainers.image.title"))
            .ok_or_else(|| anyhow!("layer missing title annotation"))?;
        info!(%name, digest = %layer.digest, "pulling layer");
        let dst = staging.join(name);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent).ok();
        }
        let mut async_file = tokio::fs::File::create(&dst)
            .await
            .with_context(|| format!("create {name}"))?;
        client
            .pull_blob(&reference, layer, &mut async_file)
            .await
            .with_context(|| format!("pull layer {name}"))?;
        async_file.flush().await.ok();
        info!(%name, "pulled");
    }
    Ok(())
}

fn verify_sha256sums(staging: &Path) -> Result<()> {
    let sums_path = staging.join("SHA256SUMS");
    let content = match fs::read_to_string(&sums_path) {
        Ok(c) => c,
        Err(_) => {
            warn!("SHA256SUMS missing, skipping verification");
            return Ok(());
        }
    };
    for line in content.lines() {
        let mut parts = line.splitn(2, "  ");
        let expected = parts.next().ok_or_else(|| anyhow!("malformed SHA256SUMS line"))?;
        let name = parts.next().ok_or_else(|| anyhow!("malformed SHA256SUMS line"))?;
        let path = staging.join(name);
        let bytes = fs::read(&path).with_context(|| format!("read {name}"))?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected {
            return Err(anyhow!("checksum mismatch for {name}"));
        }
        info!(%name, "verified");
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
