//! The permanent application lifetime owner. The managed child is transitional.
#![forbid(unsafe_code)]
mod inbox;
use crate::{
    bridge::{self, Frame},
    platform,
};
use inbox::{Event, Inbox};
use std::{
    ffi::OsString,
    io,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

struct ManagedChild {
    process: Child,
    _lifetime: platform::ChildLifetime,
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if self.process.try_wait().ok().flatten().is_none() {
            let _ = self.process.kill();
        }
        let _ = self.process.wait();
    }
}
pub fn run(mut args: Vec<OsString>) -> Result<i32, String> {
    platform::initialize()?;
    let directory = std::env::current_exe()
        .map_err(|_| "cannot locate mcw")?
        .parent()
        .ok_or("cannot locate application directory")?
        .to_path_buf();
    let mut bootstrap = Vec::new();
    loop {
        let (status, handoff) =
            host(&directory, &args, &bootstrap).map_err(|error| error.to_string())?;
        if platform::shutdown_requested() {
            return Ok(status);
        }
        match handoff {
            Some((bridge::RESTART, arguments)) => {
                args = arguments.into_iter().map(OsString::from).collect();
                bootstrap.clear();
            }
            Some((bridge::CRASH, arguments)) => {
                args = vec!["crashreport".into()];
                bootstrap = bridge::encode_strings(&arguments);
            }
            Some((bridge::UPDATE, arguments)) if status == 0 => {
                platform::start_installer(&arguments[0])
                    .map_err(|_| "could not start verified update installer")?;
                return Ok(status);
            }
            _ => return Ok(status),
        }
    }
}

type Handoff = Option<(u16, Vec<String>)>;
fn host(
    directory: &std::path::Path,
    args: &[OsString],
    bootstrap: &[u8],
) -> io::Result<(i32, Handoff)> {
    let names = ["magicalcryptowallet", "MagicalCryptoWallet.Fluent.Desktop"];
    let extension = if cfg!(windows) { ".exe" } else { "" };
    let path: PathBuf = names
        .iter()
        .map(|name| directory.join(format!("{name}{extension}")))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "managed application is missing beside mcw",
            )
        })?;
    let mut command = Command::new(path);
    command
        .args(args)
        .env("MCW_HOSTED", "1")
        .env("MCW_HOST_PATH", std::env::current_exe()?)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    platform::configure_child(&mut command);
    let mut process = command.spawn()?;
    let lifetime = match platform::ChildLifetime::new(&process) {
        Ok(lifetime) => lifetime,
        Err(error) => {
            let _ = process.kill();
            let _ = process.wait();
            return Err(error);
        }
    };
    let mut child = ManagedChild {
        process,
        _lifetime: lifetime,
    };
    let mut output = child.process.stdout.take().unwrap();
    let mut input = child.process.stdin.take().unwrap();
    // Keep reading cancellation/EOF even when application work is backlogged.
    let receive = Arc::new(Inbox::default());
    let reader_inbox = Arc::clone(&receive);
    std::thread::spawn(move || {
        loop {
            match Frame::read(&mut output) {
                Ok(Some(frame)) => {
                    if !reader_inbox.push(frame) {
                        break;
                    }
                }
                Ok(None) => {
                    reader_inbox.close(None);
                    break;
                }
                Err(error) => {
                    reader_inbox.close(Some(error));
                    break;
                }
            }
        }
    });
    let mut ready = false;
    let mut handoff = None;
    let mut broken = false;
    let mut closing = false;
    let mut stopping = None;
    let handshake_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.process.try_wait()? {
            // Drain already queued handoffs even if the process exited immediately.
            while let Event::Frame(frame) = receive.receive(Duration::ZERO) {
                // The child waits for each handoff acknowledgement before exiting.
                // A final reply write may meet an already closed input pipe.
                let _ = dispatch(
                    &frame,
                    &mut input,
                    &mut ready,
                    &mut handoff,
                    &mut closing,
                    bootstrap,
                );
            }
            return Ok((
                if status.success()
                    && (broken
                        || !ready
                        || (!closing && handoff.is_none() && !platform::shutdown_requested()))
                {
                    1
                } else {
                    status.code().unwrap_or(1)
                },
                handoff,
            ));
        }
        if (platform::shutdown_requested() || (!ready && Instant::now() > handshake_deadline))
            && stopping.is_none()
        {
            broken |= !ready;
            let shutdown = Frame {
                kind: bridge::REQUEST,
                id: 0,
                operation: bridge::SHUTDOWN,
                payload: Vec::new(),
            };
            let _ = shutdown.write(&mut input);
            stopping = Some(Instant::now());
        }
        if stopping.is_some_and(|start: Instant| start.elapsed() > Duration::from_secs(120)) {
            child.process.kill()?;
            return Ok((1, None));
        }
        match receive.receive(Duration::from_millis(50)) {
            Event::Frame(frame) => {
                if let Err(error) = dispatch(
                    &frame,
                    &mut input,
                    &mut ready,
                    &mut handoff,
                    &mut closing,
                    bootstrap,
                ) {
                    eprintln!("mcw: {error}");
                    broken = true;
                    let _ = Frame {
                        kind: bridge::REQUEST,
                        id: 0,
                        operation: bridge::SHUTDOWN,
                        payload: Vec::new(),
                    }
                    .write(&mut input);
                    stopping.get_or_insert(Instant::now());
                }
            }
            Event::Closed(failure) => {
                if let Some(failure) = failure {
                    eprintln!("mcw: {}", failure.error);
                    broken = true;
                    if let Some(reply) = failure.reply {
                        let _ = reply.write(&mut input);
                    }
                }
                // Unannounced EOF is the same failure signal observed by the child.
                // Do not return early and orphan a child still cleaning up its wallet.
                if stopping.is_none() {
                    broken |= !closing;
                    let _ = Frame {
                        kind: bridge::REQUEST,
                        id: 0,
                        operation: bridge::SHUTDOWN,
                        payload: Vec::new(),
                    }
                    .write(&mut input);
                    stopping = Some(Instant::now());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Event::Timeout => (),
        }
    }
}

fn dispatch(
    frame: &Frame,
    output: &mut impl io::Write,
    ready: &mut bool,
    handoff: &mut Handoff,
    closing: &mut bool,
    bootstrap: &[u8],
) -> io::Result<()> {
    if !*ready {
        if frame.kind != bridge::HELLO
            || frame.id != 0
            || frame.operation != 0
            || !frame.payload.is_empty()
        {
            return Err(bridge::invalid("expected bridge handshake"));
        }
        frame.reply(bootstrap.to_vec()).write(output)?;
        *ready = true;
        return Ok(());
    }
    if frame.kind == bridge::CANCEL
        && frame.id != 0
        && (frame.operation == bridge::QR || frame.operation >= 0x0100)
        && frame.payload.is_empty()
    {
        return Ok(());
    }
    if frame.kind != bridge::REQUEST || frame.id == 0 {
        return Err(bridge::invalid("unexpected bridge message"));
    }
    match frame.operation {
        bridge::SHUTDOWN if frame.payload.is_empty() => {
            *closing = true;
            frame.reply(Vec::new()).write(output)
        }
        bridge::QR => bridge::encode_qr(frame).write(output),
        bridge::RESTART | bridge::CRASH | bridge::UPDATE => {
            let args = match bridge::strings(&frame.payload) {
                Ok(args) => args,
                Err(_) => return frame.error(1, "invalid lifecycle handoff").write(output),
            };
            if handoff.is_some()
                || (frame.operation == bridge::UPDATE
                    && (args.len() != 1 || !std::path::Path::new(&args[0]).is_absolute()))
            {
                return frame.error(1, "invalid lifecycle handoff").write(output);
            }
            *handoff = Some((frame.operation, args));
            frame.reply(Vec::new()).write(output)
        }
        _ => frame.error(3, "unsupported bridge operation").write(output),
    }
}
