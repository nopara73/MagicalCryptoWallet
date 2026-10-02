// Development-only native service transport and interruption test driver.
use mcw::{
    platform::safe_file::NativeFileSystem,
    safe_file_service::{Boundary, Observer, WriteService, payload::Dispatch},
};
use std::{
    io::{self, Read, Write},
    path::Path,
};
struct CrashAt(String);
impl Observer for CrashAt {
    fn reached(&mut self, b: Boundary) -> io::Result<()> {
        if format!("{b:?}") == self.0 {
            std::process::exit(71);
        }
        Ok(())
    }
}
fn main() -> io::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).is_some_and(|s| s == "crash") {
        let mut service = WriteService::new(NativeFileSystem);
        let mut observer = CrashAt(args[3].clone());
        let id = service
            .begin_observed(Path::new(&args[2]), 12, &mut observer)
            .map_err(io::Error::other)?;
        service
            .append_observed(id, 0, b"new-complete", &mut observer)
            .map_err(io::Error::other)?;
        return service
            .commit_observed(id, &mut observer)
            .map_err(io::Error::other);
    }
    let mut dispatch = Dispatch::new(NativeFileSystem);
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    loop {
        let mut prefix = [0u8; 4];
        if input.read(&mut prefix[..1])? == 0 {
            break;
        }
        input.read_exact(&mut prefix[1..])?;
        let length = u32::from_le_bytes(prefix) as usize;
        if !(16..=1_048_576).contains(&length) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "test frame size",
            ));
        }
        let mut frame = vec![0u8; length];
        input.read_exact(&mut frame)?;
        if u16::from_le_bytes(frame[..2].try_into().unwrap()) != 1
            || frame[3] != 0
            || frame[14..16] != [0, 0]
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "test frame header",
            ));
        }
        let request = u64::from_le_bytes(frame[4..12].try_into().unwrap());
        let operation = u16::from_le_bytes(frame[12..14].try_into().unwrap());
        if frame[2] == 5 {
            dispatch.cancel(request);
            continue;
        }
        if frame[2] != 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "test frame kind",
            ));
        }
        let response = dispatch.request(request, operation, &frame[16..]);
        frame.truncate(16);
        frame[2] = 2;
        frame.extend(response);
        output.write_all(&(frame.len() as u32).to_le_bytes())?;
        output.write_all(&frame)?;
        output.flush()?;
    }
    dispatch.close();
    Ok(())
}
