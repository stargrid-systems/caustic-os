use iced::task::{Straw, sipper};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn flash_image(path: String, target: String) -> impl Straw<(), f32, Error> {
    sipper(async move |mut progress| {
        let mut input = tokio::fs::File::open(&path).await?;
        let file_size = input.metadata().await?.len();

        let mut output = open_target_device(&target).await?;

        let _ = progress.send(0.0).await;

        let mut buf = vec![0u8; 4 * 1024 * 1024];
        let mut written: u64 = 0;

        loop {
            let n = input.read(&mut buf).await?;
            if n == 0 {
                break;
            }

            output.write_all(&buf[..n]).await?;
            written += n as u64;

            let pct = 100.0 * written as f32 / file_size as f32;
            let _ = progress.send(pct).await;
        }

        output.flush().await?;

        let _ = progress.send(100.0).await;
        Ok(())
    })
}

#[cfg(target_os = "linux")]
async fn open_target_device(target: &str) -> Result<tokio::fs::File, Error> {
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .await
        .map_err(Error::from)
}

#[cfg(target_os = "windows")]
async fn open_target_device(target: &str) -> Result<tokio::fs::File, Error> {
    tokio::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .await
        .map_err(Error::from)
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
        .map_err(Error::from)
}

#[derive(Debug, Clone)]
pub enum Error {
    IoFailed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IoFailed(s) => write!(f, "{s}"),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoFailed(err.to_string())
    }
}
