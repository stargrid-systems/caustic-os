#[cfg(target_os = "linux")]
pub(super) use self::linux::{flash_elevated, prepare};
#[cfg(target_os = "macos")]
pub(super) use self::macos::{flash_elevated, prepare};
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
pub(super) use self::other::{flash_elevated, prepare};
#[cfg(target_os = "windows")]
pub use self::windows::run_privileged_child;
#[cfg(target_os = "windows")]
pub(super) use self::windows::{flash_elevated, prepare};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod other;
#[cfg(target_os = "windows")]
mod windows;
