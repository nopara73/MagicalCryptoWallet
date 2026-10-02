use std::{io, os::windows::ffi::OsStrExt, path::Path};
#[link(name = "kernel32")]
unsafe extern "system" {
    fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    fn DeleteFileW(path: *const u16) -> i32;
    fn GetFullPathNameW(
        path: *const u16,
        length: u32,
        buffer: *mut u16,
        part: *mut *mut u16,
    ) -> u32;
    fn SetFileInformationByHandle(
        handle: *mut std::ffi::c_void,
        class: i32,
        information: *const std::ffi::c_void,
        size: u32,
    ) -> i32;
}
pub fn preallocate(file: &std::fs::File, total: u64) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    struct Allocation {
        size: i64,
    }
    let allocation = Allocation {
        size: i64::try_from(total).map_err(|_| io::Error::from_raw_os_error(87))?,
    };
    const FILE_ALLOCATION_INFO: i32 = 5;
    // SAFETY: File owns the live handle and the C-compatible allocation structure
    // remains readable throughout the synchronous FileAllocationInfo (5) call.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FILE_ALLOCATION_INFO,
            (&allocation as *const Allocation).cast(),
            std::mem::size_of::<Allocation>() as u32,
        )
    } != 0
    {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if matches!(error.raw_os_error(), Some(112 | 223 | 87)) {
        Err(error)
    } else {
        Ok(())
    }
}
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let mut text = path.as_os_str().encode_wide().collect::<Vec<_>>();
    if text.contains(&0) || text.len() > 32766 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "safe file path",
        ));
    }
    if !text.starts_with(&[92, 92, 63, 92]) {
        text.push(0);
        let mut normalized = vec![0u16; 32768];
        // SAFETY: input/output buffers are live and sized in UTF-16 code units;
        // the optional final-component pointer is unused. Input is absolute.
        let length = unsafe {
            GetFullPathNameW(
                text.as_ptr(),
                normalized.len() as u32,
                normalized.as_mut_ptr(),
                std::ptr::null_mut(),
            )
        } as usize;
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        if length >= normalized.len() {
            return Err(io::Error::from_raw_os_error(206));
        }
        normalized.truncate(length);
        text = normalized;
    }
    let mut out = if text.starts_with(&[92, 92, 63, 92]) {
        text
    } else if text.starts_with(&[92, 92]) {
        let mut out = "\\\\?\\UNC\\".encode_utf16().collect::<Vec<_>>();
        out.extend_from_slice(&text[2..]);
        out
    } else {
        let mut out = "\\\\?\\".encode_utf16().collect::<Vec<_>>();
        out.extend(text);
        out
    };
    out.push(0);
    Ok(out)
}
pub fn move_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source)?;
    let destination = wide(destination)?;
    // Never COPY_ALLOWED or REPLACE_EXISTING: both names are in one directory.
    // SAFETY: each pointer is a live NUL-terminated UTF-16 buffer for this call.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 8) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub fn remove_file(path: &Path) -> io::Result<()> {
    let path = wide(path)?;
    // .NET File.Delete refuses read-only files. Rust std remove_file currently
    // permits their removal on Windows, so use the documented native operation.
    // SAFETY: the pointer is a live NUL-terminated UTF-16 buffer for this call.
    if unsafe { DeleteFileW(path.as_ptr()) } != 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(2) {
        Ok(())
    } else {
        Err(error)
    }
}
