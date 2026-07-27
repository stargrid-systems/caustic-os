#![allow(dead_code)]

use std::process::Command;

pub fn find_rpiboot() -> Option<String> {
    let name = if cfg!(target_os = "windows") {
        "rpiboot.exe"
    } else {
        "rpiboot"
    };

    which::which(name).ok().map(|p| p.to_string_lossy().into_owned())
}

pub fn run_rpiboot() -> Result<(), String> {
    let path = find_rpiboot().ok_or("rpiboot not found")?;
    let output = Command::new(&path)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }

    Ok(())
}
