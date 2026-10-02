//! The permanent application lifetime owner. The managed child is transitional.
#![forbid(unsafe_code)]
mod inbox;
use crate::{
    bridge::{self, Frame},
    platform,
};
use inbox::{Cleanup, Event, Inbox};
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
#[derive(Default)]
struct Connection {
    ready: bool,
    handoff: Handoff,
    closing: bool,
}
type SafeFiles = crate::safe_file_service::payload::Dispatch<platform::safe_file::NativeFileSystem>;
struct Services {
    psbt: crate::psbt_metadata_service::Transfers,
    safe_files: SafeFiles,
    scan: crate::scan_service::wire::Runtime,
    control_scope: Option<crate::tor_control::service::ChildScope>,
}
impl Services {
    fn new() -> Self {
        Self {
            psbt: Default::default(),
            safe_files: SafeFiles::new(platform::safe_file::NativeFileSystem),
            scan: Default::default(),
            control_scope: Some(crate::tor_control::service::ChildScope::new()),
        }
    }
    fn cancel(&mut self, id: u64, operation: u16, cleanup: Cleanup) {
        if cleanup == Cleanup::Unknown {
            return;
        }
        if (0x0600..=0x0606).contains(&operation) {
            self.psbt.cancel(id);
        }
        if (0x1000..=0x1004).contains(&operation) {
            self.safe_files.cancel(id);
        }
        if crate::scan_service::wire::handles(operation) {
            self.scan.reader().release(id, operation);
        }
        match cleanup {
            Cleanup::Psbt(session) => {
                let mut payload = vec![1, 0];
                payload.extend_from_slice(&session.to_le_bytes());
                let _ = self
                    .psbt
                    .handle(id, crate::psbt_metadata_service::ABORT, &payload);
            }
            Cleanup::SafeFile(session) => {
                let mut payload = vec![1, 0];
                payload.extend_from_slice(&session.to_le_bytes());
                // The preserved typed prefix identifies a queued session even
                // when its new correlation ID never reached the file service.
                self.safe_files.cancel_pending(operation, &payload);
            }
            Cleanup::Tor(session) => {
                let _ = crate::tor_control::service::dispatch(
                    crate::tor_control::service::CLOSE,
                    &session.to_le_bytes(),
                );
            }
            Cleanup::Scan(ticket) => self.scan.abort(ticket),
            Cleanup::Unknown | Cleanup::Request => (),
        }
    }
    fn close(&mut self) {
        self.psbt.clear();
        self.safe_files.close();
        self.scan.reader().disconnected();
        self.control_scope.take();
    }
}
impl Drop for Services {
    fn drop(&mut self) {
        self.close();
    }
}
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
    let mut services = Services::new();
    let scan_reader = services.scan.reader();
    let reader_inbox = Arc::clone(&receive);
    std::thread::spawn(move || {
        loop {
            match Frame::read(&mut output) {
                Ok(Some(frame)) => {
                    if crate::scan_service::wire::handles(frame.operation) {
                        if frame.kind == bridge::REQUEST
                            && scan_reader.register(frame.id, frame.operation).is_err()
                        {
                            reader_inbox.close(Some(bridge::invalid(
                                "invalid QR scanner request registration",
                            )));
                            break;
                        }
                        if frame.kind == bridge::CANCEL && frame.id != 0 && frame.payload.is_empty()
                        {
                            scan_reader.cancel(frame.id, frame.operation);
                        }
                    }
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
        scan_reader.disconnected();
    });
    let mut connection = Connection::default();
    let mut broken = false;
    let mut stopping = None;
    let handshake_deadline = Instant::now() + Duration::from_secs(15);
    loop {
        services.scan.reap();
        if let Some(status) = child.process.try_wait()? {
            // Drain already queued handoffs even if the process exited immediately.
            while let Event::Frame(frame) = receive.receive(Duration::ZERO) {
                // The child waits for each handoff acknowledgement before exiting.
                // A final reply write may meet an already closed input pipe.
                let _ = dispatch(
                    &frame,
                    &mut input,
                    &mut connection,
                    bootstrap,
                    &mut services,
                    &receive,
                );
                if let Some(cleanup) = receive.finish(frame.id, frame.operation) {
                    services.cancel(frame.id, frame.operation, cleanup);
                }
            }
            return Ok((
                if status.success()
                    && (broken
                        || !connection.ready
                        || (!connection.closing
                            && connection.handoff.is_none()
                            && !platform::shutdown_requested()))
                {
                    1
                } else {
                    status.code().unwrap_or(1)
                },
                connection.handoff.take(),
            ));
        }
        if (platform::shutdown_requested()
            || (!connection.ready && Instant::now() > handshake_deadline))
            && stopping.is_none()
        {
            broken |= !connection.ready;
            // Release native sessions before asking the managed owner to finish
            // its shutdown. Its cleanup may need to reopen a .new recovery file.
            services.close();
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
                    &mut connection,
                    bootstrap,
                    &mut services,
                    &receive,
                ) {
                    services.close();
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
                if let Some(cleanup) = receive.finish(frame.id, frame.operation) {
                    services.cancel(frame.id, frame.operation, cleanup);
                }
            }
            Event::Closed(failure) => {
                services.close();
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
                    broken |= !connection.closing;
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
    connection: &mut Connection,
    bootstrap: &[u8],
    services: &mut Services,
    inbox: &Inbox,
) -> io::Result<()> {
    let Connection {
        ready,
        handoff,
        closing,
    } = connection;
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
        services.cancel(
            frame.id,
            frame.operation,
            inbox.cancel_cleanup(frame.id, frame.operation),
        );
        return Ok(());
    }
    if frame.kind != bridge::REQUEST || frame.id == 0 {
        return Err(bridge::invalid("unexpected bridge message"));
    }
    match frame.operation {
        bridge::SHUTDOWN if frame.payload.is_empty() => {
            *closing = true;
            services.close();
            frame.reply(Vec::new()).write(output)
        }
        bridge::QR => bridge::encode_qr(frame).write(output),
        bridge::COMPACT_FILTER_MATCH_ANY => bridge::match_basic_compact_filter(frame).write(output),
        bridge::SCRIPT_TEXT_PARSE | bridge::SCRIPT_TEXT_RENDER => {
            bridge::encode_script_text(frame).write(output)
        }
        crate::bitcoin_address_service::VALIDATE_ADDRESS => {
            crate::bitcoin_address_service::handle(frame).write(output)
        }
        crate::nostr_event_id::OPERATION => match crate::nostr_event_id::digest(&frame.payload) {
            Ok(digest) => frame.reply(digest.to_vec()).write(output),
            Err(_) => frame
                .error(1, "invalid Nostr event ID request")
                .write(output),
        },
        crate::bitcoin_block_service::HASH_HEADER => {
            match crate::bitcoin_block_service::hash_header(&frame.payload) {
                Ok(hash) => frame.reply(hash.to_vec()).write(output),
                Err(_) => frame
                    .error(1, "invalid Bitcoin block header request")
                    .write(output),
            }
        }
        operation if crate::wallet_hash_service::handles(operation) => {
            match crate::wallet_hash_service::execute(operation, &frame.payload) {
                Ok(response) => frame.reply(response.as_bytes().to_vec()).write(output),
                Err(error) => frame.error(error.code(), error.message()).write(output),
            }
        }
        crate::psbt_metadata_service::ENRICH..=crate::psbt_metadata_service::ABORT => {
            match services
                .psbt
                .handle(frame.id, frame.operation, &frame.payload)
            {
                Ok(payload) => frame.reply(payload).write(output),
                Err(error) => frame.error(error.code(), error.message()).write(output),
            }
        }
        crate::safe_file_service::payload::BEGIN..=crate::safe_file_service::payload::PREPARE => {
            frame
                .reply(
                    services
                        .safe_files
                        .request(frame.id, frame.operation, &frame.payload),
                )
                .write(output)
        }
        crate::round_hash::OPERATION => {
            match crate::round_hash::handle(frame.operation, &frame.payload) {
                Ok(hash) => frame.reply(hash).write(output),
                Err(_) => frame
                    .error(1, "invalid round fingerprint request")
                    .write(output),
            }
        }
        crate::content_service::adapter::OPERATION => {
            let payload =
                match crate::content_service::adapter::execute(&frame.payload, &mut || {
                    if *closing
                        || platform::shutdown_requested()
                        || inbox.is_interrupted(frame.id, frame.operation)
                    {
                        Err(crate::content_service::Abort::Cancelled)
                    } else {
                        Ok(())
                    }
                }) {
                    Ok(payload) => payload,
                    Err(_) => {
                        return frame
                            .error(0x0901, "content response allocation failed")
                            .write(output);
                    }
                };
            frame.reply(payload).write(output)
        }
        crate::tor_control::service::PARSE_REPLY
        | crate::tor_control::service::PARSE_LINE
        | crate::tor_control::service::BEGIN
        | crate::tor_control::service::FEED
        | crate::tor_control::service::CLOSE => {
            let cancelled = inbox.cancellation(frame.id, frame.operation);
            match crate::tor_control::service::dispatch_control(
                frame.operation,
                &frame.payload,
                &cancelled,
            ) {
                Ok(payload) => frame.reply(payload).write(output),
                Err(error) => frame
                    .error(
                        if matches!(
                            error,
                            crate::tor_control::service::ServiceError::Interrupted
                        ) {
                            4
                        } else {
                            1
                        },
                        "Tor control codec request failed",
                    )
                    .write(output),
            }
        }
        crate::socks5::probe_service::PROBE => {
            let cancelled = crate::socks5::transport::Cancellation::from_flag(
                inbox.cancellation(frame.id, frame.operation),
            );
            match crate::socks5::probe_service::execute(frame.operation, &frame.payload, &cancelled)
            {
                Ok(response) => frame.reply(response.to_vec()).write(output),
                Err(error) => frame.error(error.code(), error.message()).write(output),
            }
        }
        operation if crate::scan_service::wire::handles(operation) => {
            match services.scan.handle(frame.id, operation, &frame.payload) {
                Ok(payload) => frame.reply(payload).write(output),
                Err(error) => frame.error(2, &error.to_string()).write(output),
            }
        }
        crate::markdown::OPERATION => {
            let cancelled = inbox.cancellation(frame.id, frame.operation);
            match crate::markdown::dispatch(&frame.payload, &cancelled) {
                Ok(payload) => frame.reply(payload).write(output),
                Err(error) => frame
                    .error(
                        match error {
                            crate::markdown::Error::InvalidInput => 1,
                            crate::markdown::Error::Limit => 2,
                            crate::markdown::Error::Cancelled => 4,
                        },
                        error.diagnostic(),
                    )
                    .write(output),
            }
        }
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

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    fn interrupt(inbox: &Inbox, terminal: u8, id: u64, operation: u16) {
        match terminal {
            0 => assert!(inbox.push(cancel(id, operation))),
            1 => inbox.close(None),
            _ => {
                for queued in id + 1..=id + 256 {
                    assert!(inbox.push(request(queued, bridge::QR, vec![1, b'x'])));
                }
                assert!(!inbox.push(request(id + 257, bridge::QR, vec![])));
            }
        }
    }

    #[test]
    fn actual_markdown_progress_observes_reader_cancel_eof_and_overload() {
        for terminal in 0..3 {
            let inbox = Arc::new(Inbox::default());
            let mut payload = vec![crate::markdown::SCHEMA];
            payload.extend_from_slice("plain text ".repeat(10_000).as_bytes());
            assert!(inbox.push(request(1, crate::markdown::OPERATION, payload)));
            let (progress_tx, progress_rx) = mpsc::sync_channel(1);
            let (resume_tx, resume_rx) = mpsc::sync_channel(1);
            let worker_inbox = Arc::clone(&inbox);
            let worker = std::thread::spawn(move || {
                let mut paused = false;
                crate::markdown::with_progress_observer(
                    move |checkpoint| {
                        if !paused && checkpoint.inline_byte_offset >= 64 {
                            paused = true;
                            assert!(checkpoint.charged_work > checkpoint.inline_byte_offset);
                            progress_tx.send(checkpoint.inline_byte_offset).unwrap();
                            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                    },
                    || dispatch_next(&mut Services::new(), &worker_inbox).unwrap(),
                )
            });
            assert!(progress_rx.recv_timeout(Duration::from_secs(5)).unwrap() >= 64);
            interrupt(&inbox, terminal, 1, crate::markdown::OPERATION);
            resume_tx.send(()).unwrap();
            let reply = worker.join().unwrap();
            assert_eq!(reply.kind, bridge::ERROR);
            assert_eq!(&reply.payload[..2], &4u16.to_le_bytes());
        }
    }

    #[test]
    fn actual_tor_feed_progress_observes_reader_interrupt_and_closes_session() {
        use crate::tor_control::{control::Point, service};
        for terminal in 0..3 {
            let inbox = Arc::new(Inbox::default());
            let session = 42u64;
            let mut begin = session.to_le_bytes().to_vec();
            begin.push(0);
            assert!(inbox.push(request(1, service::BEGIN, begin)));
            let (progress_tx, progress_rx) = mpsc::sync_channel(1);
            let (resume_tx, resume_rx) = mpsc::sync_channel(1);
            let worker_inbox = Arc::clone(&inbox);
            let worker = std::thread::spawn(move || {
                let mut services = Services::new();
                assert_eq!(
                    dispatch_next(&mut services, &worker_inbox).unwrap().kind,
                    bridge::RESPONSE
                );
                let mut feed = session.to_le_bytes().to_vec();
                feed.push(0);
                feed.extend_from_slice(b"250 ");
                feed.extend(std::iter::repeat_n(b'x', 8000));
                feed.extend_from_slice(b"\r\n");
                assert!(worker_inbox.push(request(2, service::FEED, feed)));
                let mut paused = false;
                let reply = crate::tor_control::control::observe(
                    move |point, progress| {
                        if !paused && point == Point::Scan && progress >= 4096 {
                            paused = true;
                            progress_tx.send(progress).unwrap();
                            resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        }
                    },
                    || dispatch_next(&mut services, &worker_inbox).unwrap(),
                );
                let mut abandoned = session.to_le_bytes().to_vec();
                abandoned.extend_from_slice(&[1, b'x']);
                assert_eq!(
                    service::dispatch(service::FEED, &abandoned),
                    Err(service::ServiceError::InvalidRequest)
                );
                reply
            });
            assert!(progress_rx.recv_timeout(Duration::from_secs(5)).unwrap() >= 4096);
            interrupt(&inbox, terminal, 2, service::FEED);
            resume_tx.send(()).unwrap();
            let reply = worker.join().unwrap();
            assert_eq!(reply.kind, bridge::ERROR);
            assert_eq!(&reply.payload[..2], &4u16.to_le_bytes());
        }
    }

    #[test]
    fn discarded_safe_file_writes_close_streams_without_promoting_partial_data() {
        use crate::safe_file_service::payload;
        for operation in payload::APPEND..=payload::ABORT {
            for terminal in 0..3 {
                let directory = std::env::temp_dir().join(format!(
                    "mcw-safe-queue-{}-{operation}-{terminal}",
                    std::process::id()
                ));
                std::fs::create_dir(&directory).unwrap();
                let path = directory.join("synthetic.wallet");
                std::fs::write(&path, b"original").unwrap();
                let mut services = Services::new();
                let inbox = Inbox::default();
                let text = path.to_str().unwrap().as_bytes();
                let mut begin = vec![1, 0];
                begin.extend_from_slice(&4u64.to_le_bytes());
                begin.extend_from_slice(&0u64.to_le_bytes());
                begin.push(0);
                begin.extend_from_slice(&(text.len() as u32).to_le_bytes());
                begin.extend_from_slice(text);
                assert!(inbox.push(request(1, payload::BEGIN, begin)));
                let reply = dispatch_next(&mut services, &inbox).unwrap();
                assert_eq!(reply.payload[2], 0);
                let session = u64::from_le_bytes(reply.payload[3..11].try_into().unwrap());
                let mut queued = vec![1, 0];
                queued.extend_from_slice(&session.to_le_bytes());
                queued.extend_from_slice(b"discarded queued bytes");
                assert!(inbox.push(request(2, operation, queued)));
                match terminal {
                    0 => {
                        assert!(inbox.push(cancel(2, operation)));
                        assert!(dispatch_next(&mut services, &inbox).is_none());
                    }
                    1 => {
                        inbox.close(None);
                        services.close();
                    }
                    _ => {
                        for id in 3..=257 {
                            assert!(inbox.push(request(id, bridge::QR, vec![1, b'x'])));
                        }
                        assert!(!inbox.push(request(258, bridge::QR, vec![])));
                        services.close();
                    }
                }
                assert_eq!(services.safe_files.service.active_count(), 0);
                assert_eq!(std::fs::read(&path).unwrap(), b"original");
                assert_eq!(
                    std::fs::read(path.with_extension("wallet.new")).unwrap(),
                    b""
                );
                assert!(!path.with_extension("wallet.old").exists());
                services.close();
                std::fs::remove_dir_all(directory).unwrap();
            }
        }
    }

    fn scan_header(ticket: u64) -> Vec<u8> {
        let mut payload = vec![1, 0, 0, 0];
        payload.extend_from_slice(&ticket.to_le_bytes());
        payload
    }
    fn stage_image(services: &mut Services, inbox: &Inbox) {
        use crate::scan_service::wire;
        let mut begin = scan_header(42);
        for value in [32u32; 3] {
            begin.extend_from_slice(&value.to_le_bytes());
        }
        services.scan.reader().register(1, wire::BEGIN).unwrap();
        assert!(inbox.push(request(1, wire::BEGIN, begin)));
        assert_eq!(
            dispatch_next(services, inbox).unwrap().kind,
            bridge::RESPONSE
        );
    }
    #[test]
    fn discarded_scanner_requests_release_registration_and_upload_ticket() {
        use crate::scan_service::wire;
        for operation in wire::BEGIN..=wire::ABORT {
            let mut services = Services::new();
            let inbox = Inbox::default();
            stage_image(&mut services, &inbox);
            services.scan.reader().register(2, operation).unwrap();
            assert!(inbox.push(request(2, operation, scan_header(42))));
            assert!(inbox.push(cancel(2, operation)));
            assert!(dispatch_next(&mut services, &inbox).is_none());
            assert!(!services.scan.reader().release(2, operation));
            // Beginning the same ticket again proves that the earlier upload
            // was released rather than only its new request registration.
            let mut begin = scan_header(42);
            for value in [32u32; 3] {
                begin.extend_from_slice(&value.to_le_bytes());
            }
            services.scan.reader().register(3, wire::BEGIN).unwrap();
            assert!(inbox.push(request(3, wire::BEGIN, begin)));
            assert_eq!(
                dispatch_next(&mut services, &inbox).unwrap().kind,
                bridge::RESPONSE
            );
        }
    }

    #[test]
    fn active_scanner_finish_observes_reader_cancel_eof_and_overload() {
        use crate::scan_service::wire;
        for terminal in 0..3 {
            let inbox = Arc::new(Inbox::default());
            let mut services = Services::new();
            stage_image(&mut services, &inbox);
            let mut append = scan_header(42);
            append.extend_from_slice(&0u32.to_le_bytes());
            append.extend(std::iter::repeat_n(255u8, 1024));
            services.scan.reader().register(2, wire::APPEND).unwrap();
            assert!(inbox.push(request(2, wire::APPEND, append)));
            assert_eq!(
                dispatch_next(&mut services, &inbox).unwrap().kind,
                bridge::RESPONSE
            );
            let reader = services.scan.reader();
            reader.register(3, wire::FINISH).unwrap();
            assert!(inbox.push(request(3, wire::FINISH, scan_header(42))));
            let (progress_tx, progress_rx) = mpsc::sync_channel(1);
            let (resume_tx, resume_rx) = mpsc::sync_channel(1);
            let worker_inbox = Arc::clone(&inbox);
            let worker = std::thread::spawn(move || {
                crate::scan_service::raster::on_next_threshold_start(move || {
                    progress_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                });
                dispatch_next(&mut services, &worker_inbox).unwrap()
            });
            progress_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            interrupt(&inbox, terminal, 3, wire::FINISH);
            if terminal == 0 {
                reader.cancel(3, wire::FINISH);
            } else {
                reader.disconnected();
            }
            resume_tx.send(()).unwrap();
            assert_eq!(worker.join().unwrap().kind, bridge::ERROR);
            assert!(!reader.release(3, wire::FINISH));
        }
    }

    fn request(id: u64, operation: u16, payload: Vec<u8>) -> Frame {
        Frame {
            kind: bridge::REQUEST,
            id,
            operation,
            payload,
        }
    }
    fn cancel(id: u64, operation: u16) -> Frame {
        Frame {
            kind: bridge::CANCEL,
            id,
            operation,
            payload: vec![],
        }
    }
    fn dispatch_next(services: &mut Services, inbox: &Inbox) -> Option<Frame> {
        let Event::Frame(frame) = inbox.receive(Duration::ZERO) else {
            panic!("missing frame")
        };
        let mut bytes = Vec::new();
        let mut connection = Connection {
            ready: true,
            ..Default::default()
        };
        dispatch(&frame, &mut bytes, &mut connection, &[], services, inbox).unwrap();
        if let Some(cleanup) = inbox.finish(frame.id, frame.operation) {
            services.cancel(frame.id, frame.operation, cleanup);
        }
        Frame::read(&mut Cursor::new(bytes)).unwrap()
    }
    fn begin_psbt(services: &mut Services, inbox: &Inbox) -> u64 {
        let mut payload = vec![1, 0];
        payload.extend_from_slice(&crate::psbt_metadata_service::INSPECT.to_le_bytes());
        payload.extend_from_slice(&6u32.to_le_bytes());
        assert!(inbox.push(request(1, crate::psbt_metadata_service::BEGIN, payload)));
        let reply = dispatch_next(services, inbox).unwrap();
        assert_eq!(reply.kind, bridge::RESPONSE);
        assert_eq!(services.psbt.active_sessions(), 1);
        u64::from_le_bytes(reply.payload[2..10].try_into().unwrap())
    }

    #[test]
    fn discarded_psbt_session_operations_close_the_actual_prior_transfer() {
        for operation in crate::psbt_metadata_service::APPEND..=crate::psbt_metadata_service::ABORT
        {
            let mut services = Services::new();
            let inbox = Inbox::default();
            let session = begin_psbt(&mut services, &inbox);
            let mut payload = vec![1, 0];
            payload.extend_from_slice(&session.to_le_bytes());
            payload.extend_from_slice(b"discarded private packet bytes");
            assert!(inbox.push(request(2, operation, payload)));
            // An operation mismatch cannot cancel another operation's transfer.
            assert!(inbox.push(cancel(2, operation + 1)));
            assert!(dispatch_next(&mut services, &inbox).is_none());
            assert_eq!(services.psbt.active_sessions(), 1);
            assert!(inbox.push(cancel(2, operation)));
            assert!(dispatch_next(&mut services, &inbox).is_none());
            assert_eq!(services.psbt.active_sessions(), 0);
            assert!(matches!(inbox.receive(Duration::ZERO), Event::Timeout));
            assert!(inbox.push(cancel(2, operation)));
            assert!(dispatch_next(&mut services, &inbox).is_none());
            assert_eq!(services.psbt.active_sessions(), 0);
        }
    }

    #[test]
    fn late_begin_and_unknown_cancellations_are_typed_and_idempotent() {
        let mut services = Services::new();
        let inbox = Inbox::default();
        begin_psbt(&mut services, &inbox);
        for (id, op) in [
            (999, crate::psbt_metadata_service::BEGIN),
            (1, crate::psbt_metadata_service::APPEND),
        ] {
            assert!(inbox.push(cancel(id, op)));
            assert!(dispatch_next(&mut services, &inbox).is_none());
            assert_eq!(services.psbt.active_sessions(), 1);
        }
        assert!(inbox.push(cancel(1, crate::psbt_metadata_service::BEGIN)));
        assert!(dispatch_next(&mut services, &inbox).is_none());
        assert_eq!(services.psbt.active_sessions(), 0);
    }

    #[test]
    fn active_request_token_observes_cancel_eof_and_overload_before_next_dispatch() {
        for terminal in 0..3 {
            let inbox = Inbox::default();
            assert!(inbox.push(request(1, crate::markdown::OPERATION, vec![1, b'x'])));
            let Event::Frame(frame) = inbox.receive(Duration::ZERO) else {
                panic!("missing request")
            };
            let token = inbox.cancellation(frame.id, frame.operation);
            assert!(!token.load(Ordering::Acquire));
            match terminal {
                0 => {
                    assert!(inbox.push(cancel(frame.id, frame.operation)));
                }
                1 => inbox.close(None),
                _ => {
                    for id in 2..=257 {
                        assert!(inbox.push(request(id, bridge::QR, vec![1, b'x'])));
                    }
                    assert!(!inbox.push(request(258, bridge::QR, vec![])));
                }
            }
            let mut output = Vec::new();
            let mut services = Services::new();
            let mut connection = Connection {
                ready: true,
                ..Default::default()
            };
            dispatch(
                &frame,
                &mut output,
                &mut connection,
                &[],
                &mut services,
                &inbox,
            )
            .unwrap();
            let reply = Frame::read(&mut Cursor::new(output)).unwrap().unwrap();
            assert_eq!(reply.kind, bridge::ERROR);
            assert_eq!(&reply.payload[..2], &4u16.to_le_bytes());
            assert!(token.load(Ordering::Acquire));
            assert!(inbox.finish(frame.id, frame.operation).is_some());
            services.close();
            assert_eq!(services.psbt.active_sessions(), 0);
        }
    }

    #[test]
    fn completed_request_ids_cannot_be_reused_and_receipts_are_bounded() {
        let mut services = Services::new();
        let inbox = Inbox::default();
        for id in 1..=1024 {
            assert!(inbox.push(request(id, 0xffff, vec![])));
            assert_eq!(
                dispatch_next(&mut services, &inbox).unwrap().kind,
                bridge::ERROR
            );
        }
        assert!(!inbox.push(request(1, crate::scan_service::wire::BEGIN, vec![])));
        assert!(matches!(
            inbox.receive(Duration::ZERO),
            Event::Closed(Some(_))
        ));
    }
}
