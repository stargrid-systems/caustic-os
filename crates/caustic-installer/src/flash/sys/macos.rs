use std::path::PathBuf;
use std::sync::Arc;

use crate::flash::Error;

pub async fn prepare(target: &str) {
    let disk = target.replace("/dev/rdisk", "/dev/disk");
    let _ = tokio::process::Command::new("diskutil")
        .args(["unmountDisk", &disk])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;
}

pub async fn flash_elevated(
    _image: PathBuf,
    _target: &str,
    _file_size: u64,
    _progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    Err(Error(
        "Permission denied. Run the installer as administrator.".to_string(),
    ))
}
