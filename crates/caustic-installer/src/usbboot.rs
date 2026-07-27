use std::path::Path;
use std::process::Stdio;

#[derive(Debug)]
pub enum Error {
    NotFound,
    NoElevator,
    ExecutionFailed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "rpiboot not found"),
            Self::NoElevator => write!(f, "pkexec is required for privilege escalation"),
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

pub async fn run_rpiboot() -> Result<(), Error> {
    let binary = which::which(binary_name()).map_err(|_| Error::NotFound)?;

    match run_command(&mut tokio::process::Command::new(&binary)).await {
        Ok(()) => Ok(()),
        #[cfg(target_os = "linux")]
        Err(e) if should_retry_elevated(&e) => {
            let mut cmd = build_elevated_command(&binary)?;
            run_command(&mut cmd).await
        }
        Err(e) => Err(e),
    }
}

async fn run_command(cmd: &mut tokio::process::Command) -> Result<(), Error> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| Error::ExecutionFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let msg = if stderr.is_empty() { stdout } else { stderr };
        return Err(Error::ExecutionFailed(msg));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
fn should_retry_elevated(err: &Error) -> bool {
    if is_root() {
        return false;
    }

    let Error::ExecutionFailed(msg) = err else {
        return false;
    };

    let lower = msg.to_lowercase();
    lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("could not claim interface")
        || lower.contains("access denied")
        || lower.contains("libusb_error_access")
}

#[cfg(target_os = "linux")]
fn build_elevated_command(binary: &Path) -> Result<tokio::process::Command, Error> {
    if which::which("pkexec").is_ok() {
        let mut cmd = tokio::process::Command::new("pkexec");
        cmd.arg(binary);
        return Ok(cmd);
    }

    Err(Error::NoElevator)
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    rustix::process::geteuid().as_raw() == 0
}
