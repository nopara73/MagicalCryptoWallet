//! Bounded private-pipe ingress. Reading never waits for application dispatch.
#![forbid(unsafe_code)]
use crate::bridge::{self, Frame};
use std::{
    collections::VecDeque,
    io,
    sync::{Condvar, Mutex},
    time::Duration,
};

const MAX_REQUESTS: usize = 256;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
struct State {
    requests: VecDeque<Frame>,
    cancellations: VecDeque<Frame>,
    bytes: usize,
    closed: bool,
    failure: Option<Failure>,
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
                .or_else(|| self.requests.pop_front())
        };
        match frame {
            Some(frame) => {
                if frame.kind != bridge::CANCEL {
                    self.bytes -= bridge::HEADER + frame.payload.len();
                }
                Event::Frame(frame)
            }
            None => Event::Timeout,
        }
    }
}

impl Inbox {
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
            if let Some(index) = state.requests.iter().position(|request| {
                request.kind == bridge::REQUEST
                    && request.id == frame.id
                    && request.operation == frame.operation
            }) {
                let request = state.requests.remove(index).unwrap();
                state.bytes -= bridge::HEADER + request.payload.len();
            }
            // Preserve a bounded control event for active/future service cleanup.
            if state
                .cancellations
                .iter()
                .any(|cancel| cancel.id == frame.id && cancel.operation == frame.operation)
            {
                return true;
            }
            if state.cancellations.len() < MAX_REQUESTS {
                state.cancellations.push_back(frame);
                self.changed.notify_one();
                return true;
            }
        } else {
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
