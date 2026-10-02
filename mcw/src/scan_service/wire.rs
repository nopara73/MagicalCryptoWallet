//! Scanner-owned payload adapter. Shared bridge frames/dispatch remain host-owned.
//! The owner drops all transfer state on connection end; its reader handle can
//! signal an in-progress decode while the application dispatcher is busy.
use super::{Control, Error, Result, decoder::Decoder};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

pub const BEGIN: u16 = 0x1300;
pub const APPEND: u16 = 0x1301;
pub const FINISH: u16 = 0x1302;
pub const ABORT: u16 = 0x1303;
pub const VERSION: u16 = 1;
const MAX_REQUESTS: usize = 128;
pub fn handles(op: u16) -> bool {
    (BEGIN..=ABORT).contains(&op)
}
fn invalid() -> Error {
    Error::Invalid("Invalid QR decode request")
}
struct Request {
    operation: u16,
    cancelled: Arc<AtomicBool>,
    running: bool,
}
#[derive(Default)]
struct Requests {
    entries: HashMap<u64, Request>,
    last_id: u64,
}
#[derive(Default)]
struct State {
    decoder: Decoder,
    requests: Mutex<Requests>,
    stopped: AtomicBool,
}
pub struct Runtime {
    state: Arc<State>,
}
#[derive(Clone)]
pub struct Reader {
    state: Arc<State>,
}
impl Default for Runtime {
    fn default() -> Self {
        Self {
            state: Arc::new(State::default()),
        }
    }
}
impl Runtime {
    pub fn reader(&self) -> Reader {
        Reader {
            state: self.state.clone(),
        }
    }
    pub fn reap(&self) {
        self.state.decoder.reap();
    }
    /// Idempotent cleanup of a typed upload ticket when queued work is discarded.
    /// Request registration is released separately through Reader::release.
    pub fn abort(&self, ticket: u64) {
        self.state.decoder.abort(ticket);
    }
    pub fn handle(&self, id: u64, operation: u16, payload: &[u8]) -> Result<Vec<u8>> {
        let cancelled = {
            let mut requests = self.state.requests.lock().map_err(|_| invalid())?;
            let request = requests
                .entries
                .get_mut(&id)
                .filter(|r| r.operation == operation && !r.running)
                .ok_or_else(invalid)?;
            request.running = true;
            request.cancelled.clone()
        };
        let result = self.execute(
            operation,
            payload,
            Control::new(&cancelled, Instant::now() + Duration::from_secs(2)),
        );
        if let Ok(mut requests) = self.state.requests.lock() {
            requests.entries.remove(&id);
        }
        result
    }
    fn execute(&self, op: u16, payload: &[u8], control: Control<'_>) -> Result<Vec<u8>> {
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        control.check()?;
        if payload.len() < 12
            || u16::from_le_bytes(payload[..2].try_into().unwrap()) != VERSION
            || payload[2..4] != [0, 0]
        {
            return Err(invalid());
        }
        let ticket = u64::from_le_bytes(payload[4..12].try_into().unwrap());
        if ticket == 0 {
            return Err(invalid());
        }
        let mut reply = VERSION.to_le_bytes().to_vec();
        reply.extend_from_slice(&[0, 0]);
        match op {
            BEGIN if payload.len() == 24 => {
                let w = u32::from_le_bytes(payload[12..16].try_into().unwrap()) as usize;
                let h = u32::from_le_bytes(payload[16..20].try_into().unwrap()) as usize;
                let s = u32::from_le_bytes(payload[20..24].try_into().unwrap()) as usize;
                self.state.decoder.begin(ticket, w, h, s)?;
            }
            APPEND if payload.len() > 16 => {
                let offset = u32::from_le_bytes(payload[12..16].try_into().unwrap()) as usize;
                self.state.decoder.append(ticket, offset, &payload[16..])?;
            }
            ABORT if payload.len() == 12 => self.state.decoder.abort(ticket),
            FINISH if payload.len() == 12 => {
                if let Some(d) = self.state.decoder.finish(ticket, control)? {
                    reply[2] = 1;
                    reply.extend_from_slice(&[d.version, d.level]);
                    reply.extend_from_slice(&(d.corrected_symbols as u16).to_le_bytes());
                    if let Some(p) = d.structured {
                        reply.extend_from_slice(&[p.index, p.total, p.parity, 1]);
                    } else {
                        reply.extend_from_slice(&[0, 0, 0, 0]);
                    }
                    reply.extend_from_slice(&(d.text.len() as u32).to_le_bytes());
                    reply.extend_from_slice(d.text.as_bytes());
                }
            }
            _ => return Err(invalid()),
        }
        if let Err(e) = control.check() {
            self.state.decoder.abort(ticket);
            return Err(e);
        }
        Ok(reply)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.state.stopped.store(true, Ordering::Release);
        if let Ok(mut requests) = self.state.requests.lock() {
            for request in requests.entries.values() {
                request.cancelled.store(true, Ordering::Release);
            }
            requests.entries.clear();
        }
        self.state.decoder.clear();
    }
}
impl Reader {
    /// Connection EOF/failure is a final cancellation and pixel cleanup event.
    pub fn disconnected(&self) {
        self.state.stopped.store(true, Ordering::Release);
        if let Ok(mut requests) = self.state.requests.lock() {
            for request in requests.entries.values() {
                request.cancelled.store(true, Ordering::Release);
            }
            requests.entries.clear();
        }
        self.state.decoder.clear();
    }
    /// Called for a scan request immediately after shared Frame::read, before
    /// enqueueing it. Scanner IDs must increase in wire arrival order; gaps for
    /// other operations are allowed. The shared writer allocates IDs atomically
    /// with writing. A scalar high-water mark prevents completed/discarded-ID
    /// reuse without retaining an unbounded set of historical IDs.
    pub fn register(&self, id: u64, operation: u16) -> Result<()> {
        if id == 0 || !handles(operation) {
            return Err(invalid());
        }
        let mut requests = self.state.requests.lock().map_err(|_| invalid())?;
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if id <= requests.last_id {
            return Err(invalid());
        }
        // Consume even a capacity-rejected ID. A retry needs a fresh ID.
        requests.last_id = id;
        if requests.entries.len() >= MAX_REQUESTS {
            return Err(Error::Capacity);
        }
        requests.entries.insert(
            id,
            Request {
                operation,
                cancelled: Arc::new(AtomicBool::new(false)),
                running: false,
            },
        );
        Ok(())
    }
    /// Late cancellation is harmless under the enforced increasing-ID contract.
    /// An operation mismatch never cancels an active request.
    pub fn cancel(&self, id: u64, operation: u16) {
        if let Ok(requests) = self.state.requests.lock()
            && let Some(request) = requests
                .entries
                .get(&id)
                .filter(|r| r.operation == operation)
        {
            request.cancelled.store(true, Ordering::Release);
        }
    }
    /// Deregister only the matching request when the host discards queued work.
    /// Signal before removal so an already-running handle retains cancellation.
    /// Missing/completed IDs and operation mismatches are harmless false results.
    pub fn release(&self, id: u64, operation: u16) -> bool {
        let Ok(mut requests) = self.state.requests.lock() else {
            return false;
        };
        let Some(request) = requests
            .entries
            .get(&id)
            .filter(|r| r.operation == operation)
        else {
            return false;
        };
        request.cancelled.store(true, Ordering::Release);
        requests.entries.remove(&id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    fn header(id: u64) -> Vec<u8> {
        let mut b = vec![1, 0, 0, 0];
        b.extend_from_slice(&id.to_le_bytes());
        b
    }
    fn invoke(runtime: &Runtime, id: u64, op: u16, b: &[u8]) -> Result<Vec<u8>> {
        runtime.reader().register(id, op)?;
        runtime.handle(id, op, b)
    }
    fn begin(ticket: u64, side: u32) -> Vec<u8> {
        let mut bytes = header(ticket);
        for value in [side, side, side] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
    fn append(ticket: u64, pixels: &[u8]) -> Vec<u8> {
        let mut bytes = header(ticket);
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(pixels);
        bytes
    }
    #[test]
    fn discarded_queued_requests_release_matching_registration_and_ticket() {
        for operation in [BEGIN, APPEND, FINISH, ABORT] {
            let runtime = Runtime::default();
            let reader = runtime.reader();
            let ticket = 731;
            if operation != BEGIN {
                invoke(&runtime, 1, BEGIN, &begin(ticket, 32)).unwrap();
            }
            if operation == FINISH {
                invoke(&runtime, 2, APPEND, &append(ticket, &[255; 1024])).unwrap();
            }
            reader.register(3, operation).unwrap();
            let flag = runtime.state.requests.lock().unwrap().entries[&3]
                .cancelled
                .clone();
            let wrong = if operation == BEGIN { FINISH } else { BEGIN };
            assert!(!reader.release(3, wrong));
            assert!(!flag.load(Ordering::Acquire));
            reader.cancel(3, operation);
            runtime.abort(ticket);
            assert!(reader.release(3, operation));
            assert!(flag.load(Ordering::Acquire));
            assert!(!reader.release(3, operation));
            runtime.abort(ticket);
            runtime.abort(0);
            assert_eq!(runtime.state.decoder.count(), 0);
            assert!(runtime.state.requests.lock().unwrap().entries.is_empty());
            assert!(runtime.handle(3, operation, &header(ticket)).is_err());
            assert!(reader.register(3, operation).is_err());
            invoke(&runtime, 4, BEGIN, &begin(ticket, 32)).unwrap();
            invoke(&runtime, 5, APPEND, &append(ticket, &[255; 1024])).unwrap();
            assert_eq!(
                invoke(&runtime, 6, FINISH, &header(ticket)).unwrap(),
                [1, 0, 0, 0]
            );
        }
    }
    #[test]
    fn completed_released_and_capacity_rejected_ids_cannot_be_reused() {
        let runtime = Runtime::default();
        let reader = runtime.reader();
        invoke(&runtime, 10, ABORT, &header(731)).unwrap();
        assert!(reader.register(10, ABORT).is_err());
        assert!(reader.register(10, BEGIN).is_err());
        assert!(runtime.handle(10, ABORT, &header(731)).is_err());
        reader.register(20, BEGIN).unwrap();
        let flag = runtime.state.requests.lock().unwrap().entries[&20]
            .cancelled
            .clone();
        reader.cancel(10, BEGIN);
        assert!(!reader.release(10, BEGIN));
        assert!(!flag.load(Ordering::Acquire));
        assert!(reader.release(20, BEGIN));
        assert!(reader.register(20, BEGIN).is_err());
        assert!(reader.register(19, FINISH).is_err());
        for id in 21..21 + MAX_REQUESTS as u64 {
            reader.register(id, BEGIN).unwrap();
        }
        let rejected = 21 + MAX_REQUESTS as u64;
        assert_eq!(reader.register(rejected, BEGIN), Err(Error::Capacity));
        assert!(reader.release(21, BEGIN));
        assert!(reader.register(rejected, BEGIN).is_err());
        reader.register(rejected + 1, BEGIN).unwrap();
        assert_eq!(
            runtime.state.requests.lock().unwrap().entries.len(),
            MAX_REQUESTS
        );
        let runtime = Runtime::default();
        let reader = runtime.reader();
        invoke(&runtime, u64::MAX, ABORT, &header(1)).unwrap();
        assert!(reader.register(u64::MAX, BEGIN).is_err());
        assert!(reader.register(1, BEGIN).is_err());
    }
    #[derive(Clone, Copy)]
    enum FinishSignal {
        Cancel,
        Release,
        Disconnect,
    }
    fn synchronized_finish(signal: FinishSignal) {
        let runtime = Arc::new(Runtime::default());
        let reader = runtime.reader();
        let ticket = 731;
        invoke(&runtime, 1, BEGIN, &begin(ticket, 128)).unwrap();
        invoke(&runtime, 2, APPEND, &append(ticket, &[255; 16384])).unwrap();
        reader.register(3, FINISH).unwrap();
        let (started_send, started_receive) = mpsc::channel();
        let (resume_send, resume_receive) = mpsc::channel();
        let worker_runtime = runtime.clone();
        let worker = std::thread::spawn(move || {
            super::super::raster::on_next_threshold_start(move || {
                started_send.send(()).unwrap();
                resume_receive.recv_timeout(Duration::from_secs(5)).unwrap();
            });
            worker_runtime.handle(3, FINISH, &header(ticket))
        });
        started_receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        // The real threshold loop passed its control check, so this cannot be a
        // cancellation before FINISH dispatch or before decoder execution.
        assert_eq!(runtime.state.decoder.count(), 0);
        let flag = runtime.state.requests.lock().unwrap().entries[&3]
            .cancelled
            .clone();
        assert!(runtime.state.requests.lock().unwrap().entries[&3].running);
        assert!(!flag.load(Ordering::Acquire));
        assert!(runtime.handle(3, FINISH, &header(ticket)).is_err());
        assert!(!reader.release(3, APPEND));
        match signal {
            FinishSignal::Cancel => reader.cancel(3, FINISH),
            FinishSignal::Release => {
                assert!(reader.release(3, FINISH));
                runtime.abort(ticket);
            }
            FinishSignal::Disconnect => reader.disconnected(),
        }
        assert!(flag.load(Ordering::Acquire));
        resume_send.send(()).unwrap();
        assert_eq!(worker.join().unwrap(), Err(Error::Cancelled));
        assert_eq!(runtime.state.decoder.count(), 0);
        assert!(runtime.state.requests.lock().unwrap().entries.is_empty());
        if matches!(signal, FinishSignal::Disconnect) {
            assert_eq!(reader.register(4, BEGIN), Err(Error::Cancelled));
        } else {
            invoke(&runtime, 4, BEGIN, &begin(ticket, 32)).unwrap();
            invoke(&runtime, 5, APPEND, &append(ticket, &[255; 1024])).unwrap();
            assert_eq!(
                invoke(&runtime, 6, FINISH, &header(ticket)).unwrap(),
                [1, 0, 0, 0]
            );
        }
    }
    #[test]
    fn reader_cancel_interrupts_synchronized_in_progress_finish() {
        synchronized_finish(FinishSignal::Cancel);
    }
    #[test]
    fn release_interrupts_synchronized_in_progress_finish() {
        synchronized_finish(FinishSignal::Release);
    }
    #[test]
    fn disconnect_interrupts_synchronized_in_progress_finish() {
        synchronized_finish(FinishSignal::Disconnect);
    }
    #[test]
    fn actual_payload_lifecycle_rejects_malformed_and_obeys_reader_cancel() {
        let runtime = Runtime::default();
        let reader = runtime.reader();
        let mut begin = header(10);
        for n in [21u32, 21, 21] {
            begin.extend_from_slice(&n.to_le_bytes());
        }
        assert_eq!(invoke(&runtime, 1, BEGIN, &begin).unwrap(), [1, 0, 0, 0]);
        let mut append = header(10);
        append.extend_from_slice(&0u32.to_le_bytes());
        append.extend_from_slice(&[255; 441]);
        invoke(&runtime, 2, APPEND, &append).unwrap();
        reader.register(3, FINISH).unwrap();
        reader.cancel(3, FINISH);
        assert_eq!(
            runtime.handle(3, FINISH, &header(10)),
            Err(Error::Cancelled)
        );
        // A cancellation before execution leaves the upload for the explicit
        // adapter finally/ABORT; owner Drop remains a second cleanup boundary.
        invoke(&runtime, 4, ABORT, &header(10)).unwrap();
        assert_eq!(runtime.state.decoder.count(), 0);
        assert!(invoke(&runtime, 5, BEGIN, &[1, 0, 0, 0]).is_err());
        assert!(
            invoke(&runtime, 6, BEGIN, &{
                let mut x = begin.clone();
                x[2] = 1;
                x
            })
            .is_err()
        );
        invoke(&runtime, 7, BEGIN, &begin).unwrap();
        invoke(&runtime, 8, APPEND, &append).unwrap();
        assert_eq!(
            invoke(&runtime, 9, FINISH, &header(10)).unwrap(),
            [1, 0, 0, 0]
        );
        assert_eq!(runtime.state.decoder.count(), 0);
        reader.register(11, BEGIN).unwrap();
        reader.cancel(11, APPEND);
        runtime.handle(11, BEGIN, &begin).unwrap();
        let state = runtime.state.clone();
        drop(runtime);
        assert_eq!(state.decoder.count(), 0);
        assert_eq!(reader.register(12, BEGIN), Err(Error::Cancelled));
    }
}
