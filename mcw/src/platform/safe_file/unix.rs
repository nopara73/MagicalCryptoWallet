use std::{
    ffi::CString,
    fs::File,
    io,
    os::{fd::AsRawFd, unix::ffi::OsStrExt},
    path::Path,
};

#[cfg(target_os = "linux")]
#[repr(C)]
struct StatFs {
    kind: i64,
    block_size: i64,
    blocks: u64,
    free: u64,
    available: u64,
    files: u64,
    free_files: u64,
    id: [i32; 2],
    name_length: i64,
    fragment_size: i64,
    flags: i64,
    reserved: [i64; 4],
}
#[cfg(target_os = "macos")]
#[repr(C)]
struct StatFs {
    block_size: u32,
    io_size: i32,
    blocks: u64,
    free: u64,
    available: u64,
    files: u64,
    free_files: u64,
    id: [i32; 2],
    owner: u32,
    kind: u32,
    flags: u32,
    subtype: u32,
    name: [u8; 16],
    mount: [u8; 1024],
    source: [u8; 1024],
    extra_flags: u32,
    reserved: [u32; 7],
}
#[cfg(target_os = "linux")]
const _: () = assert!(std::mem::size_of::<StatFs>() == 120);
#[cfg(target_os = "macos")]
const _: () = assert!(std::mem::size_of::<StatFs>() == 2168);

unsafe extern "C" {
    fn flock(fd: i32, operation: i32) -> i32;
    #[cfg_attr(
        all(target_os = "macos", target_arch = "x86_64"),
        link_name = "fstatfs$INODE64"
    )]
    fn fstatfs(fd: i32, status: *mut StatFs) -> i32;
}
#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn renameat2(
        old_dir: i32,
        old: *const std::ffi::c_char,
        new_dir: i32,
        new: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
    fn fallocate(fd: i32, mode: i32, offset: i64, length: i64) -> i32;
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn renamex_np(old: *const std::ffi::c_char, new: *const std::ffi::c_char, flags: u32) -> i32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
}

fn retry(mut action: impl FnMut() -> i32) -> io::Result<()> {
    loop {
        if action() == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

pub fn lock_shared(file: &File) -> io::Result<bool> {
    // SAFETY: this repr(C) record consists entirely of integer/byte fields; zero
    // is valid for each. Its layout is the supported 64-bit native statfs ABI.
    let mut status: StatFs = unsafe { std::mem::zeroed() };
    // SAFETY: the live File owns fd and the full writable statfs record remains
    // alive during each synchronous call. No pointers escape.
    if retry(|| unsafe { fstatfs(file.as_raw_fd(), &mut status) }).is_err() {
        return Ok(false);
    }
    // .NET does not take a shared write lock on NFS/SMB/CIFS, or when it cannot
    // identify the filesystem. Their shared-lock semantics can reject writes.
    #[cfg(target_os = "linux")]
    if matches!(
        status.kind as u32,
        0 | 0x6969 | 0x517b | 0xff53_4d42 | 0xfe53_4d42
    ) {
        return Ok(false);
    }
    #[cfg(target_os = "macos")]
    if status.name[0] == 0
        || status.name.starts_with(b"nfs\0")
        || status.name.starts_with(b"smbfs\0")
    {
        return Ok(false);
    }
    // SAFETY: fd is live; LOCK_SH | LOCK_NB is an advisory, nonblocking lock.
    match retry(|| unsafe { flock(file.as_raw_fd(), 1 | 4) }) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Err(error),
        Err(_) => Ok(false), // .NET ignores unsupported/other advisory lock errors.
    }
}
pub fn unlock(file: &File) {
    // SAFETY: fd remains live until after Stream::drop; LOCK_UN has no pointer.
    let _ = retry(|| unsafe { flock(file.as_raw_fd(), 8) });
}
pub fn preallocate(file: &File, total: u64) -> io::Result<()> {
    let length = i64::try_from(total).map_err(|_| io::Error::from_raw_os_error(27))?;
    #[cfg(target_os = "linux")]
    // SAFETY: fd is live and offsets are nonnegative; KEEP_SIZE reserves space
    // without changing logical .new length, matching the managed writer.
    let result = retry(|| unsafe { fallocate(file.as_raw_fd(), 1, 0, length) });
    #[cfg(target_os = "macos")]
    let result = {
        #[repr(C)]
        struct Allocation {
            flags: u32,
            position: i32,
            offset: i64,
            length: i64,
            allocated: i64,
        }
        let mut allocation = Allocation {
            flags: 4,
            position: 3,
            offset: 0,
            length,
            allocated: 0,
        };
        // SAFETY: F_PREALLOCATE (42) receives a full live fstore_t record; the
        // requested F_ALLOCATEALL/F_PEOFPOSMODE reservation keeps logical length.
        retry(|| unsafe { fcntl(file.as_raw_fd(), 42, &mut allocation as *mut Allocation) })
    };
    match result {
        Err(error) if matches!(error.raw_os_error(), Some(27 | 28)) => Err(error),
        _ => Ok(()),
    }
}
pub fn move_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "safe file path"))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "safe file path"))?;
    #[cfg(target_os = "linux")]
    let result = retry(|| {
        // SAFETY: both C strings outlive this call; RENAME_NOREPLACE forbids
        // replacing the destination. Each pointer is NUL-terminated and live.
        unsafe { renameat2(-100, source.as_ptr(), -100, destination.as_ptr(), 1) }
    });
    #[cfg(target_os = "macos")]
    let result = retry(|| {
        // SAFETY: both C strings outlive this call; RENAME_EXCL forbids replacing
        // the destination. Each pointer is NUL-terminated and live.
        unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), 4) }
    });
    result
}
#[cfg(target_os = "macos")]
pub fn full_sync(file: &File) -> io::Result<()> {
    // SAFETY: fd is owned by a live File; F_FULLFSYNC takes no argument.
    retry(|| unsafe { fcntl(file.as_raw_fd(), 51) })
}
