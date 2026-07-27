use std::process::Command;

#[derive(Debug)]
pub enum Error {
    NotFound,
    ExecutionFailed(String),
}

impl Error {
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "rpiboot not found"),
            Self::ExecutionFailed(msg) => write!(f, "rpiboot failed: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

const fn binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "rpiboot.exe"
    } else {
        "rpiboot"
    }
}

#[must_use]
pub fn is_available() -> bool {
    which::which(binary_name()).is_ok()
}

pub fn run_rpiboot() -> Result<(), Error> {
    let path = which::which(binary_name())
        .map_err(|_| Error::NotFound)?
        .to_string_lossy()
        .into_owned();

    let output = Command::new(&path)
        .output()
        .map_err(|e| Error::ExecutionFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(Error::ExecutionFailed(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ));
    }

    Ok(())
}
