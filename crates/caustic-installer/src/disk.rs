pub struct Disk {
    pub name: String,
    pub path: String,
    pub size_gb: u64,
}

#[cfg(target_os = "linux")]
pub fn list_disks() -> Vec<Disk> {
    let mut disks = Vec::new();

    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        return disks;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        if name.starts_with("loop") || name.starts_with("ram") || name.starts_with("sr") {
            continue;
        }

        let dev = format!("/dev/{name}");
        let size_path = format!("/sys/block/{name}/size");

        let Ok(size_str) = std::fs::read_to_string(&size_path) else {
            continue;
        };

        let sectors: u64 = size_str.trim().parse().unwrap_or(0);
        let size_gb = sectors * 512 / 1_000_000_000;

        let model = std::fs::read_to_string(format!("/sys/block/{name}/device/model"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let label = if model.is_empty() {
            name.clone()
        } else {
            format!("{name} - {model}")
        };

        disks.push(Disk {
            name: label,
            path: dev,
            size_gb,
        });
    }

    disks
}

#[cfg(target_os = "windows")]
pub fn list_disks() -> Vec<Disk> {
    Vec::new()
}

#[cfg(target_os = "macos")]
pub fn list_disks() -> Vec<Disk> {
    Vec::new()
}
