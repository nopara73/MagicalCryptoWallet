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
}
#[derive(Default)]
struct State {
    decoder: Decoder,
    requests: Mutex<HashMap<u64, Request>>,
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
    pub fn handle(&self, id: u64, operation: u16, payload: &[u8]) -> Result<Vec<u8>> {
        let cancelled = {
            let requests = self.state.requests.lock().map_err(|_| invalid())?;
            let request = requests
                .get(&id)
                .filter(|r| r.operation == operation)
                .ok_or_else(invalid)?;
            request.cancelled.clone()
        };
        let result = self.execute(
            operation,
            payload,
            Control::new(&cancelled, Instant::now() + Duration::from_secs(2)),
        );
        if let Ok(mut requests) = self.state.requests.lock() {
            requests.remove(&id);
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
            for request in requests.values() {
                request.cancelled.store(true, Ordering::Release);
            }
            requests.clear();
        }
        self.state.decoder.clear();
    }
}
impl Reader {
    /// Connection EOF/failure is a final cancellation and pixel cleanup event.
    pub fn disconnected(&self) {
        self.state.stopped.store(true, Ordering::Release);
        if let Ok(mut requests) = self.state.requests.lock() {
            for request in requests.values() {
                request.cancelled.store(true, Ordering::Release);
            }
            requests.clear();
        }
        self.state.decoder.clear();
    }
    /// Called for a scan request immediately after shared Frame::read, before
    /// enqueueing it. Request IDs are connection-scoped and may not be reused.
    pub fn register(&self, id: u64, operation: u16) -> Result<()> {
        if id == 0 || !handles(operation) {
            return Err(invalid());
        }
        let mut requests = self.state.requests.lock().map_err(|_| invalid())?;
        if self.state.stopped.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        if requests.len() >= MAX_REQUESTS {
            return Err(Error::Capacity);
        }
        if requests.contains_key(&id) {
            return Err(invalid());
        }
        requests.insert(
            id,
            Request {
                operation,
                cancelled: Arc::new(AtomicBool::new(false)),
            },
        );
        Ok(())
    }
    /// Late cancellation is harmless. Never cancel a reused ID/other operation.
    pub fn cancel(&self, id: u64, operation: u16) {
        if let Ok(requests) = self.state.requests.lock()
            && let Some(request) = requests.get(&id).filter(|r| r.operation == operation)
        {
            request.cancelled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn header(id: u64) -> Vec<u8> {
        let mut b = vec![1, 0, 0, 0];
        b.extend_from_slice(&id.to_le_bytes());
        b
    }
    fn invoke(runtime: &Runtime, id: u64, op: u16, b: &[u8]) -> Result<Vec<u8>> {
        runtime.reader().register(id, op)?;
        runtime.handle(id, op, b)
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
