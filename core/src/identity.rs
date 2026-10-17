//! File identity taken from an open handle, so a replaced file is detected even when
//! its size and contents happen to match (spec 1.2 "作り直しの検出").

use std::fs::File;
use std::io;

/// Volume + file number. `st_dev`/`st_ino` on Unix, volume serial + file index on Windows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileIdentity {
    pub volume: u64,
    pub index: u64,
}

#[cfg(unix)]
pub fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let meta = file.metadata()?;
    Ok(FileIdentity { volume: meta.dev(), index: meta.ino() })
}

/// `MetadataExt::volume_serial_number`/`file_index` are still unstable, so ask the
/// handle directly.
#[cfg(windows)]
pub fn file_identity(file: &File) -> io::Result<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
    };
    // SAFETY: the handle is owned by `file` and outlives the call; `info` is plain data.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, &mut info) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FileIdentity {
        volume: info.dwVolumeSerialNumber as u64,
        index: ((info.nFileIndexHigh as u64) << 32) | info.nFileIndexLow as u64,
    })
}
