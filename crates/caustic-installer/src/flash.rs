use std::path::PathBuf;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[cfg(target_os = "windows")]
pub use self::sys::run_privileged_child;
mod sys;

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

    sys::prepare(&target).await;

    match try_open_device(&target).await {
        Ok(output) => flash_direct(image, output, file_size, progress).await,
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            sys::flash_elevated(image, &target, file_size, progress).await
        }
        Err(e) => Err(Error(e.to_string())),
    }
}

async fn try_open_device(target: &str) -> std::io::Result<tokio::fs::File> {
    tokio::fs::OpenOptions::new().write(true).open(target).await
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
        written += u64::try_from(n).expect("read length fits in u64");
        progress(written, file_size);
    }

    writer.flush().await.map_err(|e| Error(e.to_string()))?;
    writer
        .get_ref()
        .sync_all()
        .await
        .map_err(|e| Error(e.to_string()))?;
    Ok(())
}

#[derive(Debug)]
pub struct Error(String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}
