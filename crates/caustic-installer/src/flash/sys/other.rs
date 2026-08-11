use std::path::PathBuf;
use std::sync::Arc;

use crate::flash::Error;

pub async fn prepare(_target: &str) {}

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
