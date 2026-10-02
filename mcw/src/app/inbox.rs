//! Bounded private-pipe ingress. Reading never waits for application dispatch.
#![forbid(unsafe_code)]
use crate::bridge::{self, Frame};
use std::{
    collections::VecDeque,
    io,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const MAX_REQUESTS: usize = 256;
const MAX_BYTES: usize = 16 * 1024 * 1024;

/// Only typed session metadata survives removal of a private request payload.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Cleanup {
    #[default]
    Unknown,
    Request,
    Psbt(u64),
    SafeFile(u64),
    Tor(u64),
    Scan(u64),
}
impl Cleanup {
    fn for_request(frame: &Frame) -> Self {
        fn session(bytes: &[u8], offset: usize) -> Option<u64> {
            let id = u64::from_le_bytes(bytes.get(offset..offset + 8)?.try_into().ok()?);
            (id != 0).then_some(id)
        }
        let version = frame.payload.get(..2) == Some(&[1, 0]);
        match frame.operation {
            0x0603..=0x0606 if version => session(&frame.payload, 2).map(Self::Psbt),
            0x1001..=0x1003 if version => session(&frame.payload, 2).map(Self::SafeFile),
            0x0F02..=0x0F04 => session(&frame.payload, 0).map(Self::Tor),
            0x1300..=0x1303 if frame.payload.get(..4) == Some(&[1, 0, 0, 0]) => {
                session(&frame.payload, 4).map(Self::Scan)
            }
            _ => None,
        }
        .unwrap_or(Self::Request)
    }
}
struct Active {
    id: u64,
    operation: u16,
    cancelled: Arc<AtomicBool>,
    cleanup: Cleanup,
}
struct Control {
    frame: Frame,
    cleanup: Cleanup,
}

#[derive(Default)]
struct State {
    requests: VecDeque<Frame>,
    cancellations: VecDeque<Control>,
    bytes: usize,
    closed: bool,
    failure: Option<Failure>,
    active: Option<Active>,
    completed: VecDeque<(u64, u16, Cleanup)>,
    delivered_cancel: Option<(u64, u16, Cleanup)>,
    last_request_id: u64,
}

pub(super) struct Failure {
    pub error: io::Error,
    pub reply: Option<Frame>,
}

pub(super) enum Event {
    Frame(Frame),
    Closed(Option<Failure>),
    Timeout,
}

#[derive(Default)]
pub(super) struct Inbox {
    state: Mutex<State>,
    changed: Condvar,
}

impl State {
    fn close(&mut self, failure: Option<Failure>) {
        if !self.closed {
            self.closed = true;
            self.failure = failure;
            self.requests.clear();
            self.cancellations.clear();
            self.bytes = 0;
            self.completed.clear();
            self.delivered_cancel = None;
            if let Some(active) = &self.active {
                active.cancelled.store(true, Ordering::Release);
            }
        }
    }

    fn next(&mut self) -> Event {
        if self.closed {
            return Event::Closed(self.failure.take());
        }
        // Preserve the handshake before processing priority control messages.
        let frame = if self
            .requests
            .front()
            .is_some_and(|f| f.kind == bridge::HELLO)
        {
            self.requests.pop_front()
        } else {
            self.cancellations
                .pop_front()
                .map(|control| {
                    self.delivered_cancel =
                        Some((control.frame.id, control.frame.operation, control.cleanup));
                    control.frame
                })
                .or_else(|| self.requests.pop_front())
        };
        match frame {
            Some(frame) => {
                if frame.kind != bridge::CANCEL {
                    self.bytes -= bridge::HEADER + frame.payload.len();
                    if frame.kind == bridge::REQUEST {
                        self.active = Some(Active {
                            id: frame.id,
                            operation: frame.operation,
                            cancelled: Arc::new(AtomicBool::new(false)),
                            cleanup: Cleanup::for_request(&frame),
                        });
                    }
                }
                Event::Frame(frame)
            }
            None => Event::Timeout,
        }
    }
}

impl Inbox {
    /// Synchronous services observe reader-side CANCEL and terminal ingress
    /// closure without waiting for dispatch to consume another event. Dispatch
    /// is the sole consumer while this query runs; request IDs pair with ops.
    pub fn is_interrupted(&self, id: u64, operation: u16) -> bool {
        let Ok(state) = self.state.lock() else {
            return true;
        };
        state.closed
            || state.active.as_ref().is_some_and(|active| {
                active.id == id
                    && active.operation == operation
                    && active.cancelled.load(Ordering::Acquire)
            })
            || state
                .cancellations
                .iter()
                .any(|cancel| cancel.frame.id == id && cancel.frame.operation == operation)
    }

    pub fn cancellation(&self, id: u64, operation: u16) -> Arc<AtomicBool> {
        let state = self.state.lock().unwrap();
        state
            .active
            .as_ref()
            .filter(|a| a.id == id && a.operation == operation)
            .map(|a| Arc::clone(&a.cancelled))
            .unwrap_or_else(|| Arc::new(AtomicBool::new(true)))
    }

    pub fn cancel_cleanup(&self, id: u64, operation: u16) -> Cleanup {
        let mut state = self.state.lock().unwrap();
        state
            .delivered_cancel
            .take()
            .filter(|(i, o, _)| *i == id && *o == operation)
            .map(|(_, _, cleanup)| cleanup)
            .unwrap_or_default()
    }

    /// Dispatch is the sole consumer. Completed receipts contain no payload and
    /// expire after 256 later operations; old/unknown CANCEL never targets work.
    pub fn finish(&self, id: u64, operation: u16) -> Option<Cleanup> {
        let mut state = self.state.lock().unwrap();
        let active = state.active.take()?;
        if active.id != id || active.operation != operation {
            state.active = Some(active);
            return None;
        }
        if !state.closed {
            if state.completed.len() == MAX_REQUESTS {
                state.completed.pop_front();
            }
            state.completed.push_back((id, operation, active.cleanup));
        }
        active
            .cancelled
            .load(Ordering::Acquire)
            .then_some(active.cleanup)
    }

    pub fn push(&self, frame: Frame) -> bool {
        let mut state = self.state.lock().unwrap();
        if state.closed {
            return false;
        }
        if frame.kind == bridge::CANCEL
            && frame.id != 0
            && (frame.operation == bridge::QR || frame.operation >= 0x0100)
            && frame.payload.is_empty()
        {
            let mut cleanup = Cleanup::Unknown;
            if let Some(index) = state.requests.iter().position(|request| {
                request.kind == bridge::REQUEST
                    && request.id == frame.id
                    && request.operation == frame.operation
            }) {
                let request = state.requests.remove(index).unwrap();
                state.bytes -= bridge::HEADER + request.payload.len();
                cleanup = Cleanup::for_request(&request);
            } else if let Some(active) = &state.active
                && active.id == frame.id
                && active.operation == frame.operation
            {
                active.cancelled.store(true, Ordering::Release);
                cleanup = active.cleanup;
            } else if let Some((_, _, completed)) = state
                .completed
                .iter()
                .find(|(id, op, _)| *id == frame.id && *op == frame.operation)
            {
                cleanup = *completed;
            }
            // Preserve a bounded control event for active/future service cleanup.
            if state.cancellations.iter().any(|cancel| {
                cancel.frame.id == frame.id && cancel.frame.operation == frame.operation
            }) {
                return true;
            }
            if state.cancellations.len() < MAX_REQUESTS {
                state.cancellations.push_back(Control { frame, cleanup });
                self.changed.notify_one();
                return true;
            }
        } else {
            if frame.kind == bridge::REQUEST && frame.id != 0 {
                // The managed writer allocates IDs under its frame-write lock.
                // Enforce non-reuse with constant space, including scanner IDs.
                if frame.id <= state.last_request_id {
                    let reply = frame.error(1, "application request ID reused or out of order");
                    state.close(Some(Failure {
                        error: bridge::invalid("application request ID reused or out of order"),
                        reply: Some(reply),
                    }));
                    self.changed.notify_one();
                    return false;
                }
                state.last_request_id = frame.id;
                if frame.operation == bridge::SHUTDOWN
                    && frame.payload.is_empty()
                    && let Some(active) = &state.active
                {
                    active.cancelled.store(true, Ordering::Release);
                }
            }
            let size = bridge::HEADER + frame.payload.len();
            if state.requests.len() < MAX_REQUESTS && size <= MAX_BYTES - state.bytes {
                state.bytes += size;
                state.requests.push_back(frame);
                self.changed.notify_one();
                return true;
            }
        }
        let reply = (frame.kind == bridge::REQUEST && frame.id != 0)
            .then(|| frame.error(4, "application request queue limit exceeded"));
        state.close(Some(Failure {
            error: bridge::invalid("application request queue limit exceeded"),
            reply,
        }));
        self.changed.notify_one();
        false
    }

    pub fn close(&self, error: Option<io::Error>) {
        let mut state = self.state.lock().unwrap();
        state.close(error.map(|error| Failure { error, reply: None }));
        self.changed.notify_one();
    }

    pub fn receive(&self, timeout: Duration) -> Event {
        let state = self.state.lock().unwrap();
        let (mut state, _) = self
            .changed
            .wait_timeout_while(state, timeout, |state| {
                !state.closed && state.requests.is_empty() && state.cancellations.is_empty()
            })
            .unwrap();
        state.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: u64, size: usize) -> Frame {
        Frame {
            kind: bridge::REQUEST,
            id,
            operation: bridge::QR,
            payload: vec![b'x'; size],
        }
    }

    #[test]
    fn cancellation_removes_saturated_queued_work_and_has_priority() {
        let inbox = Inbox::default();
        for id in 1..=MAX_REQUESTS as u64 {
            assert!(inbox.push(request(id, 1)));
        }
        let cancel = Frame {
            kind: bridge::CANCEL,
            id: 128,
            operation: bridge::QR,
            payload: vec![],
        };
        assert!(inbox.push(cancel.clone()));
        assert!(inbox.push(cancel.clone()));
        assert!(inbox.push(request(257, 1)));
        assert!(matches!(inbox.receive(Duration::ZERO), Event::Frame(f) if f == cancel));
        let mut ids = Vec::new();
        while let Event::Frame(frame) = inbox.receive(Duration::ZERO) {
            ids.push(frame.id);
        }
        assert_eq!(ids.len(), MAX_REQUESTS);
        assert!(!ids.contains(&128));
        assert_eq!(ids.last(), Some(&257));
        assert_eq!(inbox.state.lock().unwrap().bytes, 0);
    }

    #[test]
    fn saturated_eof_and_protocol_failure_discard_work_without_waiting() {
        for protocol_error in [false, true] {
            let inbox = Inbox::default();
            for id in 1..=MAX_REQUESTS as u64 {
                assert!(inbox.push(request(id, 1)));
            }
            inbox.close(protocol_error.then(|| bridge::invalid("invalid bridge header")));
            assert!(
                matches!(inbox.receive(Duration::ZERO), Event::Closed(error) if error.is_some() == protocol_error)
            );
            let state = inbox.state.lock().unwrap();
            assert!(state.requests.is_empty());
            assert!(state.cancellations.is_empty());
            assert_eq!(state.bytes, 0);
            drop(state);
            assert!(!inbox.push(request(258, 1)));
        }
    }

    #[test]
    fn count_and_byte_overloads_are_typed_and_payload_safe() {
        for bytes in [false, true] {
            let inbox = Inbox::default();
            let size = if bytes {
                bridge::MAX_FRAME - bridge::HEADER
            } else {
                1
            };
            let count = if bytes {
                MAX_BYTES / bridge::MAX_FRAME
            } else {
                MAX_REQUESTS
            };
            for id in 1..=count as u64 {
                assert!(inbox.push(request(id, size)));
            }
            let rejected = request(count as u64 + 1, size);
            assert!(!inbox.push(rejected.clone()));
            let Event::Closed(Some(failure)) = inbox.receive(Duration::ZERO) else {
                panic!("overload was not prioritized");
            };
            assert_eq!(
                failure.error.to_string(),
                "application request queue limit exceeded"
            );
            assert_eq!(
                failure.reply,
                Some(rejected.error(4, "application request queue limit exceeded"))
            );
            assert_eq!(inbox.state.lock().unwrap().bytes, 0);
        }
    }
}
