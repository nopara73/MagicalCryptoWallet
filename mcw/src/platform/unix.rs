use super::*;
unsafe extern "C" {
    fn signal(number: i32, handler: usize) -> usize;
}
extern "C" fn stop(_number: i32) {
    SHUTDOWN.store(true, Ordering::Relaxed);
}
pub fn initialize() -> Result<(), String> {
    // SAFETY: the handler only writes a lock-free atomic; it does not allocate.
    unsafe {
        signal(2, stop as *const () as usize);
        signal(15, stop as *const () as usize);
    }
    Ok(())
}
pub fn attach_console() {}
pub fn configure_child(_command: &mut Command) {}
pub struct ChildLifetime;
impl ChildLifetime {
    pub fn new(_child: &Child) -> std::io::Result<Self> {
        Ok(Self)
    }
}
pub fn start_installer(path: &str) -> std::io::Result<()> {
    let installer = std::path::Path::new(path);
    if !installer.is_absolute() || !installer.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid update installer",
        ));
    }
    #[cfg(target_os = "macos")]
    {
        Command::new("/usr/bin/open").arg(path).spawn()?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        Command::new("/usr/bin/xdg-open").arg(path).spawn()?;
    }
    Ok(())
}
