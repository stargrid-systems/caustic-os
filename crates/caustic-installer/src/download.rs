use iced::futures::StreamExt;
use iced::task::{Straw, sipper};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

pub fn download_image(
    url: String,
    dest_path: String,
    checksum_url: Option<String>,
) -> impl Straw<(), f32, Error> {
    sipper(async move |mut progress| {
        let client = reqwest::Client::builder()
            .user_agent("caustic-installer")
            .build()?;

        let expected_hash = if let Some(checksum_url) = checksum_url {
            let resp = client.get(&checksum_url).send().await?;
            let text = resp.text().await?;
            parse_checksum(&text)
        } else {
            None
        };

        let response = client.get(&url).send().await?;
        let total = response.content_length().ok_or(Error::NoContentLength)?;

        let _ = progress.send(0.0).await;

        let mut file = tokio::fs::File::create(&dest_path).await?;

        let mut byte_stream = response.bytes_stream();
        let mut downloaded: u64 = 0;
        let mut hasher = Sha256::new();

        while let Some(chunk) = byte_stream.next().await {
            let bytes = chunk?;
            hasher.update(&bytes);
            file.write_all(&bytes).await?;
            downloaded += bytes.len() as u64;
            let pct = 100.0 * downloaded as f32 / total as f32;
            let _ = progress.send(pct).await;
        }

        file.flush().await?;
        drop(file);

        if let Some(expected) = expected_hash {
            let actual: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
            if !actual.eq_ignore_ascii_case(&expected) {
                let _ = tokio::fs::remove_file(&dest_path).await;
                return Err(Error::ChecksumMismatch { expected, actual });
            }
        }

        let _ = progress.send(100.0).await;
        Ok(())
    })
}

fn parse_checksum(text: &str) -> Option<String> {
    for line in text.lines() {
        if line.ends_with(".img.xz") {
            let hash = line.split_whitespace().next()?;
            return Some(hash.to_string());
        }
    }
    None
}

#[derive(Debug, Clone)]
pub enum Error {
    RequestFailed(String),
    NoContentLength,
    IoFailed(String),
    ChecksumMismatch { expected: String, actual: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestFailed(s) => write!(f, "Request failed: {s}"),
            Self::NoContentLength => write!(f, "Server did not report content length"),
            Self::IoFailed(s) => write!(f, "I/O error: {s}"),
            Self::ChecksumMismatch { expected, actual } => {
                write!(f, "Checksum mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::RequestFailed(err.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::IoFailed(err.to_string())
    }
}
