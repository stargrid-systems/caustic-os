use std::process::Command;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemState {
    Initializing,
    Starting,
    Running,
    Degraded,
    Maintenance,
    Stopping,
    Offline,
    Unknown,
}

impl FromStr for SystemState {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim() {
            "initializing" => Ok(Self::Initializing),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "degraded" => Ok(Self::Degraded),
            "maintenance" => Ok(Self::Maintenance),
            "stopping" => Ok(Self::Stopping),
            "offline" => Ok(Self::Offline),
            "unknown" => Ok(Self::Unknown),
            other => Err(anyhow!("unrecognized system state: {other}")),
        }
    }
}

const SYSTEMCTL: &str = "/run/current-system/sw/bin/systemctl";

pub fn is_system_running() -> Result<SystemState> {
    let output = Command::new(SYSTEMCTL)
        .arg("is-system-running")
        .output()
        .context("exec systemctl is-system-running")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().parse()
}
