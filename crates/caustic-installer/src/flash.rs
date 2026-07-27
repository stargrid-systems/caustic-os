use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

const CHUNK_SIZE: usize = 4 * 1024 * 1024;
const WRITE_BUFFER_SIZE: usize = 8 * 1024 * 1024;

pub async fn flash_image(
    image: PathBuf,
    target: String,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let file_size = tokio::fs::metadata(&image)
        .await
        .map_err(|e| Error(e.to_string()))?
        .len();

    match try_open_device(&target).await {
        Ok(output) => flash_direct(image, output, file_size, progress).await,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            #[cfg(target_os = "linux")]
            {
                flash_elevated(image, &target, file_size, progress).await
            }
            #[cfg(not(target_os = "linux"))]
            {
                let _ = e;
                Err(Error(
                    "Permission denied. Run the installer as administrator.".to_string(),
                ))
            }
        }
        Err(e) => Err(Error(e.to_string())),
    }
}

async fn try_open_device(target: &str) -> std::io::Result<tokio::fs::File> {
    #[cfg(target_os = "macos")]
    let resolved = target.replace("/dev/disk", "/dev/rdisk");
    #[cfg(not(target_os = "macos"))]
    let resolved = target.to_string();

    tokio::fs::OpenOptions::new().write(true).open(&resolved).await
}

async fn flash_direct(
    image: PathBuf,
    output: tokio::fs::File,
    file_size: u64,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let mut input = tokio::fs::File::open(&image)
        .await
        .map_err(|e| Error(e.to_string()))?;

    let mut writer = tokio::io::BufWriter::with_capacity(WRITE_BUFFER_SIZE, output);

    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut written: u64 = 0;

    loop {
        let n = input
            .read(&mut buf)
            .await
            .map_err(|e| Error(e.to_string()))?;
        if n == 0 {
            break;
        }
        writer
            .write_all(&buf[..n])
            .await
            .map_err(|e| Error(e.to_string()))?;
        written += u64::try_from(n).unwrap_or(0);
        progress(written, file_size);
    }

    writer.flush().await.map_err(|e| Error(e.to_string()))?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn flash_elevated(
    image: PathBuf,
    target: &str,
    file_size: u64,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let elevator = find_elevator().ok_or_else(|| {
        Error("Neither pkexec nor sudo is available to elevate dd".to_string())
    })?;

    let if_arg = format!("if={}", image.display());
    let of_arg = format!("of={target}");

    let mut cmd = tokio::process::Command::new(elevator);
    cmd.args(["dd", &if_arg, &of_arg, "bs=4M", "conv=fsync", "status=progress"])
        .stderr(Stdio::piped())
        .stdout(Stdio::null());

    let mut child = cmd
        .spawn()
        .map_err(|e| Error(format!("Failed to start dd: {e}")))?;

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

#[cfg(target_os = "linux")]
fn find_elevator() -> Option<&'static str> {
    if which::which("pkexec").is_ok() {
        Some("pkexec")
    } else if which::which("sudo").is_ok() {
        Some("sudo")
    } else {
        None
    }
}

fn parse_dd_progress(s: &str) -> Option<u64> {
    let bytes_idx = s.find(" bytes")?;
    s[..bytes_idx]
        .split_whitespace()
        .last()?
        .parse()
        .ok()
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}
