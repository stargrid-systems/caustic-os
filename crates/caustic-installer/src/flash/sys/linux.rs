use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::AsyncBufReadExt;

use crate::flash::Error;

const REQUIRED_BINARIES: &[&str] = &["pkexec", "sh", "umount", "wipefs", "blockdev", "dd"];

pub async fn prepare(_target: &str) {}

pub async fn flash_elevated(
    image: PathBuf,
    target: &str,
    file_size: u64,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let missing = missing_binaries(|name| which::which(name).is_ok());
    if !missing.is_empty() {
        return Err(Error(format!(
            "missing required binaries: {}",
            missing.join(", ")
        )));
    }

    let children = enumerate_child_partitions(target).await;

    let mut cmd = tokio::process::Command::new("pkexec");
    cmd.arg("sh")
        .arg("-c")
        .arg(ELEVATED_FLASH_SCRIPT)
        .arg("caustic-flash")
        .arg(&image)
        .arg(target)
        .args(&children)
        .stderr(Stdio::piped())
        .stdout(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| Error(format!("Failed to start pkexec: {e}")))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error("Failed to capture dd stderr".to_string()))?;

    let mut reader = tokio::io::BufReader::new(stderr);
    let mut line = Vec::new();
    let mut last_output = String::new();

    loop {
        line.clear();
        let n = reader
            .read_until(b'\r', &mut line)
            .await
            .map_err(|e| Error(format!("Failed reading dd output: {e}")))?;

        if n == 0 {
            break;
        }

        let s = String::from_utf8_lossy(&line);
        let trimmed = s.trim();

        if trimmed.is_empty() {
            continue;
        }

        if let Some(bytes) = parse_dd_progress(trimmed) {
            progress(bytes, file_size);
        } else {
            last_output = trimmed.to_string();
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| Error(format!("Failed waiting for dd: {e}")))?;

    if !status.success() {
        return Err(Error(if last_output.is_empty() {
            format!("dd exited with status {status}")
        } else {
            last_output
        }));
    }

    Ok(())
}

fn missing_binaries(exists: impl Fn(&str) -> bool) -> Vec<&'static str> {
    REQUIRED_BINARIES
        .iter()
        .filter(|b| !exists(b))
        .copied()
        .collect()
}

const ELEVATED_FLASH_SCRIPT: &str = r#"set -e
img="$1"
dev="$2"
shift 2
for p in "$@"; do umount "$p" 2>/dev/null || true; done
wipefs -a "$dev"
blockdev --rereadpt "$dev"
dd if="$img" of="$dev" bs=4M conv=fsync status=progress
"#;

async fn enumerate_child_partitions(target: &str) -> Vec<String> {
    let output = tokio::process::Command::new("lsblk")
        .args([target, "-ln", "-o", "name"])
        .output()
        .await;
    match output {
        Ok(o) => parse_child_devices(&String::from_utf8_lossy(&o.stdout), target),
        Err(_) => Vec::new(),
    }
}

fn parse_child_devices(lsblk_output: &str, target: &str) -> Vec<String> {
    let target_name = target.trim_start_matches("/dev/");
    lsblk_output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let name = line.trim_start_matches("/dev/");
            if name == target_name {
                None
            } else {
                Some(format!("/dev/{name}"))
            }
        })
        .collect()
}

fn parse_dd_progress(s: &str) -> Option<u64> {
    s.split_whitespace().next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::{missing_binaries, parse_child_devices};

    #[test]
    fn excludes_target_and_prefixes_dev() {
        let out = "mmcblk0\nmmcblk0p1\nmmcblk0p2\n";
        let children = parse_child_devices(out, "/dev/mmcblk0");
        assert_eq!(
            children,
            vec!["/dev/mmcblk0p1".to_string(), "/dev/mmcblk0p2".to_string()]
        );
    }

    #[test]
    fn ignores_blank_lines() {
        let out = "\nsda\n\nsda1\nsda2\n";
        let children = parse_child_devices(out, "/dev/sda");
        assert_eq!(
            children,
            vec!["/dev/sda1".to_string(), "/dev/sda2".to_string()]
        );
    }

    #[test]
    fn handles_already_prefixed_input() {
        let out = "/dev/sdb\n/dev/sdb1\n";
        let children = parse_child_devices(out, "/dev/sdb");
        assert_eq!(children, vec!["/dev/sdb1".to_string()]);
    }

    #[test]
    fn empty_when_only_target() {
        assert!(parse_child_devices("loop0\n", "/dev/loop0").is_empty());
    }

    #[test]
    fn missing_binaries_reports_only_absent() {
        let missing = missing_binaries(|n| n != "wipefs");
        assert_eq!(missing, vec!["wipefs"]);
    }

    #[test]
    fn missing_binaries_none_when_all_present() {
        assert!(missing_binaries(|_| true).is_empty());
    }
}
