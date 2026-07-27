pub struct Disk {
    pub name: String,
    pub path: String,
    pub size_gb: u64,
    pub removable: bool,
}

const SECTOR_SIZE: u64 = 512;
const BYTES_PER_GB: u64 = 1_000_000_000;

#[cfg(target_os = "linux")]
pub fn list_disks() -> Vec<Disk> {
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        return Vec::new();
    };

    let boot_disks = expand_device_paths(&read_boot_disks());
    let swap_disks = expand_device_paths(&read_swap_disks());

    let mut disks = Vec::new();

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        if is_virtual_device(&name) {
            continue;
        }

        if boot_disks.iter().any(|d| device_matches_block(d, &name)) {
            continue;
        }

        if swap_disks.iter().any(|d| device_matches_block(d, &name)) {
            continue;
        }

        let dev = format!("/dev/{name}");
        let size_path = format!("/sys/block/{name}/size");

        let Ok(size_str) = std::fs::read_to_string(&size_path) else {
            continue;
        };

        let sectors: u64 = size_str.trim().parse().unwrap_or_default();
        let size_gb = sectors * SECTOR_SIZE / BYTES_PER_GB;

        if size_gb == 0 {
            continue;
        }

        let model = read_sys_block(&name, "device/model").unwrap_or_default();

        let removable = read_sys_block(&name, "removable")
            .is_some_and(|s| s == "1");

        let vendor = read_sys_block(&name, "device/vendor").unwrap_or_default();

        let label = if model.is_empty() {
            name.clone()
        } else if vendor.is_empty() {
            format!("{name} - {model}")
        } else {
            format!("{name} - {vendor} {model}")
        };

        disks.push(Disk {
            name: label,
            path: dev,
            size_gb,
            removable,
        });
    }

    disks.sort_by_key(|d| !d.removable);
    disks
}

#[cfg(target_os = "linux")]
fn is_virtual_device(name: &str) -> bool {
    const SKIP_PREFIXES: &[&str] = &["loop", "ram", "zram", "sr", "md", "dm-", "nvme-fabrics"];
    SKIP_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

#[cfg(target_os = "linux")]
fn read_sys_block(name: &str, file: &str) -> Option<String> {
    std::fs::read_to_string(format!("/sys/block/{name}/{file}"))
        .ok()
        .map(|s| s.trim().to_string())
}

#[cfg(target_os = "linux")]
fn read_boot_disks() -> Vec<String> {
    let critical_mounts = ["/", "/boot", "/boot/efi", "/usr", "/var"];
    let mut devices = Vec::new();

    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        for line in mounts.lines() {
            let mut parts = line.split_whitespace();
            let Some(source) = parts.next() else { continue };
            let Some(mountpoint) = parts.next() else { continue };

            if critical_mounts.contains(&mountpoint) && source.starts_with("/dev/") {
                devices.push(source.to_string());
            }
        }
    }

    devices
}

#[cfg(target_os = "linux")]
fn read_swap_disks() -> Vec<String> {
    let mut devices = Vec::new();

    if let Ok(swaps) = std::fs::read_to_string("/proc/swaps") {
        for line in swaps.lines().skip(1) {
            if let Some(source) = line.split_whitespace().next()
                && source.starts_with("/dev/")
            {
                devices.push(source.to_string());
            }
        }
    }

    devices
}

#[cfg(target_os = "linux")]
fn expand_device_paths(devices: &[String]) -> Vec<String> {
    let mut expanded = Vec::new();
    for dev in devices {
        let canonical = std::fs::canonicalize(dev)
            .map_or_else(|_| dev.clone(), |p| p.to_string_lossy().into_owned());

        if !expanded.contains(&canonical) {
            expanded.push(canonical.clone());
        }

        if let Some(base) = canonical.strip_prefix("/dev/")
            && let Ok(slaves) = std::fs::read_dir(format!("/sys/block/{base}/slaves"))
        {
            for entry in slaves.flatten() {
                let slave = format!("/dev/{}", entry.file_name().to_string_lossy());
                if !expanded.contains(&slave) {
                    expanded.push(slave);
                }
            }
        }
    }
    expanded
}

#[cfg(target_os = "linux")]
fn device_matches_block(dev_path: &str, block_name: &str) -> bool {
    let Some(base) = dev_path.strip_prefix("/dev/") else {
        return false;
    };
    if base == block_name {
        return true;
    }
    let Some(rest) = base.strip_prefix(block_name) else {
        return false;
    };
    !rest.is_empty() && rest.starts_with(|c: char| c.is_ascii_digit() || c == 'p')
}

#[cfg(target_os = "windows")]
pub fn list_disks() -> Vec<Disk> {
    use std::process::Command;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-PhysicalDisk | ForEach-Object { \"$($_.DeviceId)|$($_.FriendlyName)|$($_.Size)|$($_.BusType)\" }",
        ])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let boot_disk = windows_boot_disk_number();

    let mut disks = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('|').collect();
        if parts.len() != 4 {
            continue;
        }

        let id = parts[0].trim();
        let name = parts[1].trim();
        let size: u64 = parts[2].trim().parse().unwrap_or(0);
        let bus = parts[3].trim();

        if id == boot_disk {
            continue;
        }

        let size_gb = size / BYTES_PER_GB;
        if size_gb == 0 {
            continue;
        }

        disks.push(Disk {
            name: format!("{name} ({bus})"),
            path: format!("\\\\.\\PhysicalDrive{id}"),
            size_gb,
            removable: bus == "USB",
        });
    }

    disks
}

#[cfg(target_os = "windows")]
fn windows_boot_disk_number() -> String {
    use std::process::Command;

    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-Partition | Where-Object { $_.IsBoot -or $_.IsSystem } | Select-Object -First 1 -ExpandProperty DiskNumber)",
        ])
        .output();

    let Ok(output) = output else {
        return String::new();
    };

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_string()
}

#[cfg(target_os = "macos")]
pub fn list_disks() -> Vec<Disk> {
    use std::process::Command;

    let output = Command::new("diskutil")
        .args(["list", "-plist"])
        .output();

    let Ok(output) = output else {
        return Vec::new();
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut disks = Vec::new();
    for line in stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("<key>WholeDisk</key>") {
            let _ = rest;
        }
        if let Some(rest) = trimmed.strip_prefix("<string>disk") {
            let disk_num: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !disk_num.is_empty() {
                disks.push(Disk {
                    name: format!("disk{disk_num}"),
                    path: format!("/dev/rdisk{disk_num}"),
                    size_gb: 0,
                    removable: false,
                });
            }
        }
    }

    disks
}
