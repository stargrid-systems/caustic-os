use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn flash_image(
    image: PathBuf,
    target: String,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let mut input = tokio::fs::File::open(&image)
        .await
        .map_err(|e| Error(e.to_string()))?;
    let file_size = input
        .metadata()
        .await
        .map_err(|e| Error(e.to_string()))?
        .len();

    let mut output = open_target_device(&target).await?;

    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut written: u64 = 0;

    loop {
        let n = input
            .read(&mut buf)
            .await
            .map_err(|e| Error(e.to_string()))?;
        if n == 0 {
            break;
        }
        output
            .write_all(&buf[..n])
            .await
            .map_err(|e| Error(e.to_string()))?;
        written += n as u64;
        progress(written, file_size);
    }

    output
        .flush()
        .await
        .map_err(|e| Error(e.to_string()))?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn open_target_device(target: &str) -> Result<tokio::fs::File, Error> {
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .await
        .map_err(|e| Error(e.to_string()))
}

#[cfg(target_os = "windows")]
async fn open_target_device(target: &str) -> Result<tokio::fs::File, Error> {
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .await
        .map_err(|e| Error(e.to_string()))
}

#[cfg(target_os = "macos")]
async fn open_target_device(target: &str) -> Result<tokio::fs::File, Error> {
    let path = if target.starts_with("/dev/disk") {
        target.replace("/dev/disk", "/dev/rdisk")
    } else {
        target.to_string()
    };
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .await
        .map_err(|e| Error(e.to_string()))
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}
