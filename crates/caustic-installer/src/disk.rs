pub struct Disk {
    pub device: String,
    pub description: String,
    pub size: u64,
    pub is_removable: bool,
    pub bus_type: String,
}

const BYTES_PER_GB: u64 = 1_000_000_000;

impl Disk {
    #[must_use]
    pub const fn size_gb(&self) -> u64 {
        self.size / BYTES_PER_GB
    }
}

#[cfg(target_os = "linux")]
mod lsblk {
    use std::process::Command;

    use super::Disk;

    #[derive(serde::Deserialize)]
    struct Output {
        blockdevices: Vec<Device>,
    }

    #[derive(serde::Deserialize)]
    struct Device {
        name: String,
        #[serde(default)]
        size: String,
        #[serde(default)]
        rm: String,
        #[serde(default)]
        hotplug: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        vendor: Option<String>,
        #[serde(default)]
        tran: Option<String>,
        #[serde(default)]
        r#type: Option<String>,
        #[serde(default)]
        subsystems: Option<String>,
    }

    impl Device {
        fn is_virtual(&self) -> bool {
            self.subsystems.as_deref() == Some("block")
        }

        fn is_removable(&self) -> bool {
            self.rm == "1" || self.hotplug == "1"
        }

        fn is_system(&self) -> bool {
            !self.is_removable()
        }
    }

    pub fn list() -> Vec<Disk> {
        let output = Command::new("lsblk")
            .args([
                "--bytes",
                "--json",
                "--paths",
                "--output",
                "name,size,rm,hotplug,model,vendor,tran,type,subsystems",
            ])
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };

        let parsed: Output = match serde_json::from_slice(&output.stdout) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut disks: Vec<Disk> = parsed
            .blockdevices
            .into_iter()
            .filter(|d| d.r#type.as_deref() == Some("disk"))
            .filter(|d| !is_skip_device(&d.name))
            .filter(|d| !d.is_system())
            .filter(|d| !d.is_virtual())
            .map(|d| {
                let is_removable = d.is_removable();
                let device = d.name;
                Disk {
                    bus_type: d.tran.as_deref().unwrap_or("UNKNOWN").to_uppercase(),
                    description: build_description(
                        d.vendor.as_deref().unwrap_or(""),
                        d.model.as_deref().unwrap_or(""),
                        &device,
                    ),
                    device,
                    is_removable,
                    size: d.size.parse().unwrap_or(0),
                }
            })
            .filter(|d| d.size > 0)
            .collect();

        disks.sort_by_key(|d| !d.is_removable);
        disks
    }

    fn is_skip_device(name: &str) -> bool {
        let dev = name.trim_start_matches("/dev/");
        dev.starts_with("loop")
            || dev.starts_with("ram")
            || dev.starts_with("sr")
            || dev.starts_with("zram")
    }

    fn build_description(vendor: &str, model: &str, name: &str) -> String {
        let parts: Vec<&str> = [vendor.trim(), model.trim()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect();
        if parts.is_empty() {
            name.trim_start_matches("/dev/").to_string()
        } else {
            parts.join(" ")
        }
    }
}

#[cfg(target_os = "macos")]
mod diskutil {
    use std::process::Command;

    use super::Disk;

    pub fn list() -> Vec<Disk> {
        let mut disk_names = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("disk") && !name.contains('s') {
                    disk_names.push(name);
                }
            }
        }
        disk_names.sort();

        let mut disks = Vec::new();
        for disk_name in &disk_names {
            let device = format!("/dev/{disk_name}");
            if let Some(disk) = info(&device) {
                disks.push(disk);
            }
        }

        disks.sort_by_key(|d| !d.is_removable);
        disks
    }

    fn info(device: &str) -> Option<Disk> {
        let output = Command::new("diskutil")
            .args(["info", "-plist", device])
            .output()
            .ok()?;

        let plist = plist::Value::from_reader(std::io::Cursor::new(output.stdout))?;
        let dict = plist.as_dictionary()?;

        let size = dict.get("TotalSize")?.as_unsigned_integer()?;

        if size == 0 {
            return None;
        }

        let internal = dict
            .get("Internal")
            .and_then(plist::Value::as_boolean)
            .unwrap_or(true);

        let removable = dict
            .get("Removable")
            .and_then(plist::Value::as_boolean)
            .unwrap_or(false);

        let ejectable = dict
            .get("Ejectable")
            .and_then(plist::Value::as_boolean)
            .unwrap_or(false);

        let is_removable = removable || ejectable;

        if internal && !is_removable {
            return None;
        }

        let bus_type = dict
            .get("BusProtocol")
            .and_then(plist::Value::as_string)
            .unwrap_or("Unknown")
            .to_string();

        let description = dict
            .get("MediaName")
            .and_then(plist::Value::as_string)
            .unwrap_or(device)
            .to_string();

        let raw = device.replace("/dev/disk", "/dev/rdisk");

        Some(Disk {
            device: raw,
            description,
            size,
            is_removable,
            bus_type,
        })
    }
}

#[cfg(target_os = "windows")]
mod win_disk {
    use std::process::Command;

    use super::Disk;

    pub fn list() -> Vec<Disk> {
        let output = Command::new("powershell")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$sysDisks = @(Get-Partition -ErrorAction SilentlyContinue | Where-Object { \
                 $_.IsBoot -or $_.IsSystem } | Select-Object -ExpandProperty DiskNumber -Unique); \
                 Get-PhysicalDisk | ForEach-Object { [PSCustomObject]@{ DeviceId=$_.DeviceId; \
                 FriendlyName=$_.FriendlyName; Size=$_.Size; BusType=$_.BusType.ToString(); \
                 IsSystem=($sysDisks -contains ([int]$_.DeviceId)) } } | ConvertTo-Json -Compress",
            ])
            .output();

        let Ok(output) = output else {
            return Vec::new();
        };

        let stdout = String::from_utf8_lossy(&output.stdout);

        let value: serde_json::Value = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let entries = match value {
            serde_json::Value::Array(arr) => arr,
            other => vec![other],
        };

        let mut disks = Vec::new();
        for entry in entries {
            let Some(device_id) = entry.get("DeviceId").and_then(|v| v.as_str()) else {
                continue;
            };
            let friendly_name = entry
                .get("FriendlyName")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let size = entry.get("Size").and_then(|v| v.as_u64()).unwrap_or(0);
            let bus_type = entry
                .get("BusType")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            let is_system = entry
                .get("IsSystem")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let is_removable = matches!(
                bus_type,
                "USB" | "SD" | "MMC" | "1394" | "FileBackedVirtual"
            );

            if !is_install_target(is_system, size, is_removable) {
                continue;
            }

            disks.push(Disk {
                device: format!("\\\\.\\PhysicalDrive{device_id}"),
                description: friendly_name.to_string(),
                size,
                is_removable,
                bus_type: bus_type.to_string(),
            });
        }

        disks.sort_by_key(|d| !d.is_removable);
        disks
    }

    fn is_install_target(is_system: bool, size: u64, is_removable: bool) -> bool {
        !is_system && size > 0 && is_removable
    }

    #[cfg(test)]
    mod tests {
        use super::is_install_target;

        #[test]
        fn usb_disk_is_target() {
            assert!(is_install_target(false, 16_000_000_000, true));
        }

        #[test]
        fn internal_sata_disk_is_dropped() {
            assert!(!is_install_target(false, 500_000_000_000, false));
        }

        #[test]
        fn system_disk_is_dropped_even_if_removable() {
            assert!(!is_install_target(true, 16_000_000_000, true));
        }

        #[test]
        fn zero_size_disk_is_dropped() {
            assert!(!is_install_target(false, 0, true));
        }
    }
}

#[cfg(target_os = "linux")]
pub fn list_disks() -> Vec<Disk> {
    lsblk::list()
}

#[cfg(target_os = "macos")]
pub fn list_disks() -> Vec<Disk> {
    diskutil::list()
}

#[cfg(target_os = "windows")]
pub fn list_disks() -> Vec<Disk> {
    win_disk::list()
}
