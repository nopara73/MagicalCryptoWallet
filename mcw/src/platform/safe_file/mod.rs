//! Native filesystem operations for the small SafeFile writer.
use crate::safe_file_service::FileSystem;
use std::{
    fs::{self, File, OpenOptions},
    io,
    path::Path,
};
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

pub struct NativeFileSystem;
pub struct Stream {
    file: File,
    #[cfg(unix)]
    locked: bool,
}
impl io::Write for Stream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        io::Write::write(&mut self.file, bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        io::Write::flush(&mut self.file)
    }
}
impl Drop for Stream {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.locked {
            unix::unlock(&self.file);
        }
    }
}
impl FileSystem for NativeFileSystem {
    type Stream = Stream;
    fn ensure_parent(&self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "safe file parent"))?;
        #[cfg(unix)]
        let missing = parent
            .ancestors()
            .take_while(|p| !p.exists())
            .map(Path::to_path_buf)
            .collect::<Vec<_>>();
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        for created in missing.iter().rev() {
            if let Some(ancestor) = created.parent() {
                File::open(ancestor)?.sync_all()?;
            }
        }
        Ok(())
    }
    fn open_new(&self, path: &Path, disable_file_locking: bool) -> io::Result<Stream> {
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create(true)
            .truncate(cfg!(windows) || disable_file_locking);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        let file = options.open(path)?;
        #[cfg(windows)]
        let _ = disable_file_locking;
        #[cfg(unix)]
        let locked = if disable_file_locking {
            false
        } else {
            unix::lock_shared(&file)?
        };
        let stream = Stream {
            file,
            #[cfg(unix)]
            locked,
        };
        #[cfg(unix)]
        if !disable_file_locking
            && let Err(error) = stream.file.set_len(0)
            && !matches!(error.raw_os_error(), Some(9 | 22))
        {
            return Err(error);
        }
        Ok(stream)
    }
    fn preallocate(&self, stream: &Stream, total: u64) -> io::Result<()> {
        #[cfg(windows)]
        {
            windows::preallocate(&stream.file, total)
        }
        #[cfg(unix)]
        {
            unix::preallocate(&stream.file, total)
        }
    }
    fn sync_new(&self, stream: &Stream) -> io::Result<()> {
        stream.file.sync_all()?;
        #[cfg(target_os = "macos")]
        unix::full_sync(&stream.file)?;
        Ok(())
    }
    fn file_exists(&self, path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|m| !m.is_dir())
    }
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            windows::remove_file(path)
        }
        #[cfg(unix)]
        {
            match fs::remove_file(path) {
                Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
                r => r,
            }
        }
    }
    fn move_no_replace(&self, source: &Path, destination: &Path) -> io::Result<()> {
        #[cfg(windows)]
        {
            windows::move_no_replace(source, destination)
        }
        #[cfg(unix)]
        {
            unix::move_no_replace(source, destination)
        }
    }
    fn sync_parent(&self, path: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "safe file parent"))?;
            File::open(parent)?.sync_all()
        }
        // Windows uses flushed file contents and MOVEFILE_WRITE_THROUGH. Win32
        // has no documented portable directory-fsync equivalent; do not claim
        // that a process interruption test proves physical power-loss durability.
        #[cfg(windows)]
        {
            let _ = path;
            Ok(())
        }
    }
}
