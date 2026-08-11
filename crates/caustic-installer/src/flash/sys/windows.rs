// This module is Win32 FFI glue. Translating between Win32 sizes, pointers
// and Rust integers needs casts the pedantic lint would otherwise reject.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_lossless,
    clippy::cast_sign_loss,
    clippy::borrow_as_ptr,
    clippy::ref_as_ptr,
    clippy::ptr_as_ptr,
    clippy::cast_ptr_alignment
)]
use std::ffi::OsStr;
use std::iter;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_MODE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FindFirstVolumeW, FindNextVolumeW, FindVolumeClose, FlushFileBuffers,
    GetTempPathW, OPEN_EXISTING, ReadFile, WriteFile,
};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::System::Ioctl::{
    DISK_EXTENT, FSCTL_DISMOUNT_VOLUME, IOCTL_DISK_UPDATE_PROPERTIES, VOLUME_DISK_EXTENTS,
};
use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
use windows::Win32::UI::Shell::{SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows::core::PCWSTR;

use crate::flash::Error;

const CHUNK_SIZE: usize = 4 * 1024 * 1024;
const GENERIC_READ: u32 = 0x8000_0000;
const GENERIC_WRITE: u32 = 0x4000_0000;
const SW_HIDE: i32 = 0;
const IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS: u32 = 0x0056_0000;

// HANDLE wraps a raw pointer so it is not `Send`. We only store an owned
// process handle and wait on it from another thread, which is sound.
#[derive(Clone, Copy)]
struct SendHandle(HANDLE);
unsafe impl Send for SendHandle {}

pub async fn prepare(_target: &str) {}

pub async fn flash_elevated(
    image: PathBuf,
    target: &str,
    file_size: u64,
    progress: Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    let progress_file = temp_progress_path()?;
    std::fs::write(&progress_file, b"")
        .map_err(|e| Error(format!("Failed to create progress file: {e}")))?;

    let exe = std::env::current_exe()
        .map_err(|e| Error(format!("Failed to locate installer exe: {e}")))?;
    let args = build_privileged_args(&image, target, &progress_file);

    let hprocess = {
        let verb_w = to_wide("runas");
        let exe_w = to_wide(&exe.to_string_lossy());
        let params_w = to_wide(&params_string(&args));
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.hwnd = HWND(std::ptr::null_mut());
        info.lpVerb = PCWSTR(verb_w.as_ptr());
        info.lpFile = PCWSTR(exe_w.as_ptr());
        info.lpParameters = PCWSTR(params_w.as_ptr());
        info.lpDirectory = PCWSTR::null();
        info.nShow = SW_HIDE;
        if unsafe { ShellExecuteExW(&mut info) }.is_err() {
            let _ = std::fs::remove_file(&progress_file);
            return Err(Error("Elevation was declined or failed".to_string()));
        }
        SendHandle(info.hProcess)
    };

    let result = poll_progress(hprocess, &progress_file, file_size, &progress).await;

    unsafe {
        let _ = CloseHandle(hprocess.0);
    }
    let _ = std::fs::remove_file(&progress_file);

    result
}

async fn poll_progress(
    process: SendHandle,
    progress_file: &Path,
    file_size: u64,
    progress: &Arc<dyn Fn(u64, u64) + Send + Sync>,
) -> Result<(), Error> {
    loop {
        let wait = unsafe { WaitForSingleObject(process.0, 200) };

        if let Ok(contents) = std::fs::read_to_string(progress_file) {
            let state = parse_progress_file(&contents);
            if let Some(frac) = state.progress {
                progress(fraction_to_bytes(frac, file_size), file_size);
            }
            if let Some(err) = state.error {
                return Err(Error(err));
            }
        }

        if wait.0 as u32 == 0xFFFF_FFFF {
            return Err(Error("Failed to wait for elevated process".to_string()));
        }
        if wait.0 == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let mut code: u32 = 0;
    unsafe { GetExitCodeProcess(process.0, &mut code) }
        .map_err(|e| Error(format!("Failed to get exit code: {e}")))?;
    if code != 0 {
        if let Ok(contents) = std::fs::read_to_string(progress_file) {
            let state = parse_progress_file(&contents);
            if let Some(err) = state.error {
                return Err(Error(err));
            }
        }
        return Err(Error(format!("Elevated process exited with code {code}")));
    }

    progress(file_size, file_size);
    Ok(())
}

fn fraction_to_bytes(frac: f32, total: u64) -> u64 {
    (frac.clamp(0.0, 1.0) * total as f32) as u64
}

pub fn run_privileged_child(image: &str, device: &str, progress_file: &str) -> i32 {
    match try_privileged_flash(image, device, progress_file) {
        Ok(()) => 0,
        Err(msg) => {
            let _ = append_progress(progress_file, &format!("error {msg}"));
            1
        }
    }
}

fn try_privileged_flash(image: &str, device: &str, progress_file: &str) -> Result<(), String> {
    let drive_number = parse_physical_drive_number(device)
        .ok_or_else(|| format!("unsupported device path '{device}'"))?;

    let file_size = std::fs::metadata(image)
        .map_err(|e| format!("cannot stat image: {e}"))?
        .len();

    let image_h = open_handle(image, GENERIC_READ, FILE_SHARE_READ)?;
    let drive_h = open_handle(
        device,
        GENERIC_READ | GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?;

    let outcome = run_privileged_flash(image_h, drive_h, drive_number, file_size, progress_file);

    unsafe {
        let _ = CloseHandle(drive_h);
        let _ = CloseHandle(image_h);
    }
    outcome
}

fn run_privileged_flash(
    image_h: HANDLE,
    drive_h: HANDLE,
    drive_number: u32,
    file_size: u64,
    progress_file: &str,
) -> Result<(), String> {
    dismount_volumes_on_drive(drive_number);
    stream_image(image_h, drive_h, file_size, progress_file)?;

    unsafe {
        let _ = DeviceIoControl(
            drive_h,
            IOCTL_DISK_UPDATE_PROPERTIES,
            None,
            0,
            None,
            0,
            None,
            None,
        );
    }
    let _ = append_progress(progress_file, "done");
    Ok(())
}

fn stream_image(
    image_h: HANDLE,
    drive_h: HANDLE,
    file_size: u64,
    progress_file: &str,
) -> Result<(), String> {
    let mut buf = vec![0u8; CHUNK_SIZE];
    let mut written: u64 = 0;
    loop {
        let mut bytes_read: u32 = 0;
        let read = unsafe {
            ReadFile(
                image_h,
                Some(&mut buf),
                Some(&mut bytes_read as *mut u32),
                None,
            )
        };
        if read.is_err() {
            return Err("failed reading image".to_string());
        }
        if bytes_read == 0 {
            break;
        }
        let mut bytes_written: u32 = 0;
        let write = unsafe {
            WriteFile(
                drive_h,
                Some(&buf[..bytes_read as usize]),
                Some(&mut bytes_written as *mut u32),
                None,
            )
        };
        if write.is_err() || bytes_written != bytes_read {
            return Err("failed writing to device".to_string());
        }
        written += u64::from(bytes_written);
        let frac = (written as f32 / file_size as f32).min(1.0);
        let _ = append_progress(progress_file, &format!("progress {frac}"));
    }
    unsafe { FlushFileBuffers(drive_h) }.map_err(|e| format!("flush failed: {e}"))
}

fn open_handle(path: &str, access: u32, share: FILE_SHARE_MODE) -> Result<HANDLE, String> {
    let wide = to_wide(path);
    unsafe {
        CreateFileW(
            PCWSTR(wide.as_ptr()),
            access,
            share,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }
    .map_err(|e| format!("cannot open '{path}': {e}"))
}

fn dismount_volumes_on_drive(drive_number: u32) {
    let mut volume_name = [0u16; 260];
    let find = unsafe { FindFirstVolumeW(&mut volume_name) };
    let Ok(handle) = find else {
        return;
    };

    let mut name = volume_name;
    loop {
        dismount_if_on_drive(&name, drive_number);
        if unsafe { FindNextVolumeW(handle, &mut name) }.is_err() {
            break;
        }
    }

    unsafe {
        let _ = FindVolumeClose(handle);
    }
}

fn dismount_if_on_drive(volume_name: &[u16], drive_number: u32) {
    let Ok(handle) = (unsafe {
        CreateFileW(
            PCWSTR(volume_name.as_ptr()),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        )
    }) else {
        return;
    };

    if volume_is_on_drive(handle, drive_number) {
        unsafe {
            let _ = DeviceIoControl(handle, FSCTL_DISMOUNT_VOLUME, None, 0, None, 0, None, None);
        }
    }

    unsafe {
        let _ = CloseHandle(handle);
    }
}

fn volume_is_on_drive(handle: HANDLE, drive_number: u32) -> bool {
    let mut buf = vec![
        0u8;
        std::mem::size_of::<VOLUME_DISK_EXTENTS>()
            + 15 * std::mem::size_of::<DISK_EXTENT>()
    ];
    let mut bytes: u32 = 0;
    let ok = unsafe {
        DeviceIoControl(
            handle,
            IOCTL_VOLUME_GET_VOLUME_DISK_EXTENTS,
            None,
            0,
            Some(buf.as_mut_ptr() as *mut _),
            buf.len() as u32,
            Some(&mut bytes as *mut u32),
            None,
        )
    }
    .is_ok();
    if !ok {
        return false;
    }

    let extents = unsafe { &*(buf.as_ptr() as *const VOLUME_DISK_EXTENTS) };
    let count = extents.NumberOfDiskExtents;
    if count == 0 {
        return false;
    }

    let first: *const DISK_EXTENT = extents.Extents.as_ptr();
    for i in 0..count.min(16) {
        let ext = unsafe { &*first.add(i as usize) };
        if ext.DiskNumber == drive_number {
            return true;
        }
    }
    false
}

fn parse_physical_drive_number(device: &str) -> Option<u32> {
    let rest = device.strip_prefix(r"\\.\PhysicalDrive")?;
    rest.parse::<u32>().ok()
}

fn temp_progress_path() -> Result<PathBuf, Error> {
    let mut buf = [0u16; 260];
    let len = unsafe { GetTempPathW(Some(&mut buf)) };
    if len == 0 {
        return Err(Error("Failed to get temp path".to_string()));
    }
    let s = String::from_utf16_lossy(&buf[..len as usize]);
    Ok(PathBuf::from(s).join(format!("caustic-flash-{}.progress", std::process::id())))
}

fn params_string(args: &[String]) -> String {
    args.iter()
        .map(|a| quote_arg(a))
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote_arg(arg: &str) -> String {
    if arg.contains(' ') || arg.contains('"') {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

fn append_progress(progress_file: &str, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(progress_file)?;
    writeln!(f, "{line}")
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(iter::once(0)).collect()
}

fn build_privileged_args(image: &Path, device: &str, progress_file: &Path) -> Vec<String> {
    vec![
        "--privileged-flash".to_string(),
        image.to_string_lossy().into_owned(),
        device.to_string(),
        progress_file.to_string_lossy().into_owned(),
    ]
}

struct ProgressState {
    progress: Option<f32>,
    done: bool,
    error: Option<String>,
}

fn parse_progress_file(contents: &str) -> ProgressState {
    let mut state = ProgressState {
        progress: None,
        done: false,
        error: None,
    };
    for line in contents.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("progress ") {
            state.progress = rest.trim().parse::<f32>().ok();
        } else if line == "done" {
            state.done = true;
        } else if let Some(rest) = line.strip_prefix("error ") {
            state.error = Some(rest.trim().to_string());
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{build_privileged_args, parse_progress_file};

    #[test]
    fn build_privileged_args_layout() {
        let args = build_privileged_args(
            Path::new("C:/images/os.img"),
            r"\\.\PhysicalDrive2",
            Path::new("C:/tmp/p.progress"),
        );
        assert_eq!(
            args,
            vec![
                "--privileged-flash".to_string(),
                "C:/images/os.img".to_string(),
                r"\\.\PhysicalDrive2".to_string(),
                "C:/tmp/p.progress".to_string(),
            ]
        );
    }

    #[test]
    fn parse_progress_file_keeps_latest_progress() {
        let state = parse_progress_file("progress 0.5\nprogress 0.75\n");
        assert_eq!(state.progress, Some(0.75));
        assert!(!state.done);
        assert!(state.error.is_none());
    }

    #[test]
    fn parse_progress_file_done() {
        let state = parse_progress_file("progress 1\ndone\n");
        assert_eq!(state.progress, Some(1.0));
        assert!(state.done);
    }

    #[test]
    fn parse_progress_file_error() {
        let state = parse_progress_file("progress 0.3\nerror access denied\n");
        assert_eq!(state.error.as_deref(), Some("access denied"));
        assert!(!state.done);
    }

    #[test]
    fn parse_progress_file_empty() {
        let state = parse_progress_file("");
        assert!(state.progress.is_none());
        assert!(!state.done);
        assert!(state.error.is_none());
    }
}
