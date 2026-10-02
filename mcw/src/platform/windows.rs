use super::*;
use std::{
    ffi::c_void,
    io,
    os::windows::{io::AsRawHandle, process::CommandExt},
};
type Handle = *mut c_void;
#[link(name = "kernel32", kind = "raw-dylib")]
unsafe extern "system" {
    fn AttachConsole(process: u32) -> i32;
    fn GetStdHandle(kind: u32) -> Handle;
    fn SetStdHandle(kind: u32, handle: Handle) -> i32;
    fn SetConsoleCtrlHandler(handler: extern "system" fn(u32) -> i32, add: i32) -> i32;
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, length: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}
#[link(name = "shell32", kind = "raw-dylib")]
unsafe extern "system" {
    fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
}
extern "system" fn stop(_event: u32) -> i32 {
    SHUTDOWN.store(true, Ordering::Relaxed);
    1
}
pub fn attach_console() {
    // SAFETY: attach to the invoking terminal when present; redirected handles are
    // left intact by Windows when there is no parent console.
    unsafe {
        let handles = [
            (-10i32 as u32, GetStdHandle(-10i32 as u32)),
            (-11i32 as u32, GetStdHandle(-11i32 as u32)),
            (-12i32 as u32, GetStdHandle(-12i32 as u32)),
        ];
        AttachConsole(u32::MAX);
        // AttachConsole can replace inherited redirected handles. Preserve stdin
        // and stdout pipes so CLI use never becomes an interactive console read.
        for (kind, handle) in handles {
            if !handle.is_null() && handle.addr() != usize::MAX {
                SetStdHandle(kind, handle);
            }
        }
    }
}
pub fn initialize() -> Result<(), String> {
    let id: Vec<u16> = "io.github.nopara73.magicalcryptowallet"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: id is NUL-terminated and alive for the call; stop has system ABI.
    unsafe {
        SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
        SetConsoleCtrlHandler(stop, 1);
    }
    Ok(())
}
pub fn configure_child(command: &mut Command) {
    command.creation_flags(0x08000000);
}
#[repr(C)]
#[derive(Default)]
struct BasicLimit {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min: usize,
    max: usize,
    active: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimit {
    basic: BasicLimit,
    io: [u64; 6],
    process_memory: usize,
    job_memory: usize,
    peak_process: usize,
    peak_job: usize,
}
pub struct ChildLifetime(Handle);
impl ChildLifetime {
    pub fn new(child: &Child) -> io::Result<Self> {
        // SAFETY: no name/security overrides. This private job owns only our child.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = Self(handle);
        let mut info = ExtendedLimit::default();
        info.basic.flags = 0x2000;
        // SAFETY: the buffer has the documented x64 JOBOBJECT_EXTENDED_LIMIT layout;
        // the child handle is owned by Child. Drop closes our job exactly once.
        if unsafe {
            SetInformationJobObject(
                handle,
                9,
                (&info as *const ExtendedLimit).cast(),
                std::mem::size_of::<ExtendedLimit>() as u32,
            ) == 0
                || AssignProcessToJobObject(handle, child.as_raw_handle()) == 0
        } {
            return Err(io::Error::last_os_error());
        }
        Ok(result)
    }
}
impl Drop for ChildLifetime {
    fn drop(&mut self) {
        // SAFETY: this is our live job handle.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub fn start_installer(path: &str) -> io::Result<()> {
    let installer = std::path::Path::new(path);
    if !installer.is_file()
        || installer
            .extension()
            .is_none_or(|extension| !extension.eq_ignore_ascii_case("msi"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid update installer",
        ));
    }
    let system = std::env::var_os("SystemRoot")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Windows directory unavailable"))?;
    Command::new(std::path::Path::new(&system).join("System32/msiexec.exe"))
        .args(["/i", path])
        .spawn()?;
    Ok(())
}
