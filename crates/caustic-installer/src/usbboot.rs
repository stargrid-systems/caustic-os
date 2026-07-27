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
            Self::NoElevator => write!(f, "no privilege escalation tool available"),
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
    let binary = which::which(binary_name())
        .map_err(|_| Error::NotFound)?
        .to_string_lossy()
        .into_owned();

    let mut cmd = build_elevated_command(&binary)?;

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
fn build_elevated_command(binary: &str) -> Result<tokio::process::Command, Error> {
    if std::fs::OpenOptions::new()
        .read(true)
        .open("/dev/raw-gadget")
        .is_ok()
        || is_root()
    {
        return Ok(tokio::process::Command::new(binary));
    }

    if which::which("pkexec").is_ok() {
        let mut cmd = tokio::process::Command::new("pkexec");
        cmd.arg(binary);
        return Ok(cmd);
    }

    if which::which("sudo").is_ok() {
        let mut cmd = tokio::process::Command::new("sudo");
        cmd.arg(binary);
        return Ok(cmd);
    }

    Err(Error::NoElevator)
}

#[cfg(target_os = "linux")]
fn is_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.trim().parse::<u32>().ok())
        .is_some_and(|uid| uid == 0)
}

#[cfg(not(target_os = "linux"))]
fn build_elevated_command(binary: &str) -> Result<tokio::process::Command, Error> {
    Ok(tokio::process::Command::new(binary))
}
