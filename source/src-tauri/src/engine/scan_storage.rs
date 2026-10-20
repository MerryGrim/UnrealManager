//! A one-shot, read-only storage hint for the installed-file scan. Unknown
//! devices keep sequential reads; no worker, polling task or device cache lives
//! beyond this operation. Integrity checks do not depend on this hint.
use std::path::Path;

const NON_SEEKING_WORKERS: usize = 4;

fn worker_limit(seek_penalty: Option<bool>, processors: usize) -> usize {
    if seek_penalty == Some(false) {
        if processors >= NON_SEEKING_WORKERS { NON_SEEKING_WORKERS }
        else if processors >= 2 { 2 } else { 1 }
    } else {
        1
    }
}

fn decode_seek_penalty(bytes: &[u8], returned: usize) -> Option<bool> {
    // DWORD Version, DWORD Size, BOOLEAN IncursSeekPenalty, C padding.
    // Validate the raw boolean before interpreting a driver's response.
    if returned < 12 || returned > bytes.len() { return None; }
    let version = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let size = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    if version < 12 || version > returned || size < version || size > returned { return None; }
    match bytes[8] { 0 => Some(false), 1 => Some(true), _ => None }
}

pub(super) fn workers_hint(root: &Path) -> usize {
    let processors = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
    worker_limit(seek_penalty(root), processors)
}

#[cfg(not(windows))]
fn seek_penalty(_: &Path) -> Option<bool> { None }

#[cfg(windows)]
fn seek_penalty(root: &Path) -> Option<bool> {
    use std::{os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        Storage::FileSystem::{CreateFileW, GetDriveTypeW, GetVolumeNameForVolumeMountPointW,
            GetVolumePathNameW, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING},
        System::{IO::DeviceIoControl, Ioctl::{IOCTL_STORAGE_QUERY_PROPERTY,
            PropertyStandardQuery, STORAGE_PROPERTY_QUERY, StorageDeviceSeekPenaltyProperty}},
    };
    if !root.is_absolute() { return None; }
    let name: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
    if name[..name.len().saturating_sub(1)].contains(&0) { return None; }
    let mut mount = vec![0u16; 32768];
    let mut volume = [0u16; 64];
    unsafe {
        if GetVolumePathNameW(name.as_ptr(), mount.as_mut_ptr(), mount.len() as u32) == 0 {
            return None;
        }
        // DRIVE_FIXED only. Network/removable drives retain sequential reads.
        if GetDriveTypeW(mount.as_ptr()) != 3 { return None; }
        if GetVolumeNameForVolumeMountPointW(mount.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) == 0 {
            return None;
        }
        let end = volume.iter().position(|c| *c == 0)?;
        if end == 0 || volume[end - 1] != b'\\' as u16 { return None; }
        volume[end - 1] = 0;
        let handle = CreateFileW(volume.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(), OPEN_EXISTING, 0, ptr::null_mut());
        if handle == INVALID_HANDLE_VALUE || handle.is_null() { return None; }
        struct Device(HANDLE);
        impl Drop for Device { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }
        let device = Device(handle);
        let query = STORAGE_PROPERTY_QUERY {
            PropertyId: StorageDeviceSeekPenaltyProperty, QueryType: PropertyStandardQuery,
            AdditionalParameters: [0],
        };
        let mut output = [0u8; 64];
        let mut returned = 0u32;
        if DeviceIoControl(device.0, IOCTL_STORAGE_QUERY_PROPERTY,
            (&query as *const STORAGE_PROPERTY_QUERY).cast(), std::mem::size_of_val(&query) as u32,
            output.as_mut_ptr().cast(), output.len() as u32, &mut returned, ptr::null_mut()) == 0 {
            return None;
        }
        decode_seek_penalty(&output, returned as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descriptor(flag: u8) -> [u8; 12] {
        let mut bytes = [0u8; 12];
        bytes[..4].copy_from_slice(&12u32.to_le_bytes());
        bytes[4..8].copy_from_slice(&12u32.to_le_bytes());
        bytes[8] = flag; bytes
    }
    #[test]
    fn only_positive_non_seeking_hint_allows_bounded_parallelism() {
        assert_eq!(worker_limit(None, 16), 1);
        assert_eq!(worker_limit(Some(true), 16), 1);
        assert_eq!(worker_limit(Some(false), 1), 1);
        assert_eq!(worker_limit(Some(false), 0), 1);
        assert_eq!(worker_limit(Some(false), 2), 2);
        assert_eq!(worker_limit(Some(false), 3), 2);
        assert_eq!(worker_limit(Some(false), 16), NON_SEEKING_WORKERS);
    }
    #[test]
    fn device_response_requires_a_complete_valid_descriptor() {
        assert_eq!(decode_seek_penalty(&descriptor(0), 12), Some(false));
        assert_eq!(decode_seek_penalty(&descriptor(1), 12), Some(true));
        assert_eq!(decode_seek_penalty(&descriptor(2), 12), None);
        assert_eq!(decode_seek_penalty(&descriptor(0), 8), None);
        assert_eq!(decode_seek_penalty(&descriptor(0), 13), None);
        let mut invalid = descriptor(0); invalid[4..8].copy_from_slice(&64u32.to_le_bytes());
        assert_eq!(decode_seek_penalty(&invalid, 12), None);
        invalid = descriptor(0); invalid[..4].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(decode_seek_penalty(&invalid, 12), None);
    }
}
