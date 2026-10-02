// Compiled by the current-source portable verifier, or the historical review
// verifier, with exact source-hashed host Inbox/Frame modules. No Cargo target.
// This synchronized component proof complements the shipping-host wire proof.
use crate::content_service::Abort;
use crate::{
    bridge,
    content_service::adapter,
    inbox::{Event, Inbox},
};
use std::{
    io::Write,
    net::{Shutdown, TcpListener, TcpStream},
    sync::{Arc, mpsc},
    thread::{self, JoinHandle},
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(3);
const ACTIVE: u64 = 900;
const PLAIN_SIZE: usize = 128 * 1024;
const QUEUE: usize = 256;

fn frame(kind: u8, id: u64, operation: u16, payload: Vec<u8>) -> bridge::Frame {
    bridge::Frame {
        kind,
        id,
        operation,
        payload,
    }
}
fn request(id: u64) -> bridge::Frame {
    frame(bridge::REQUEST, id, adapter::OPERATION, vec![42])
}
fn cancel(id: u64, operation: u16) -> bridge::Frame {
    frame(bridge::CANCEL, id, operation, Vec::new())
}

struct Bits {
    bytes: Vec<u8>,
    offset: usize,
}
impl Bits {
    fn raw(&mut self, value: u32, count: usize) {
        for n in 0..count {
            if self.offset % 8 == 0 {
                self.bytes.push(0);
            }
            *self.bytes.last_mut().unwrap() |= (((value >> n) & 1) as u8) << (self.offset % 8);
            self.offset += 1;
        }
    }
    fn align(&mut self) {
        self.raw(0, (8 - self.offset % 8) % 8);
    }
}
/// RFC7932 uncompressed meta blocks; exact bits are independent fixture data.
fn packet() -> Vec<u8> {
    let mut bits = Bits {
        bytes: Vec::new(),
        offset: 0,
    };
    bits.raw(0, 1); // WBITS=16
    for _ in 0..2 {
        bits.raw(0, 1); // non-final
        bits.raw(0, 2); // 16-bit length
        bits.raw(65535, 16);
        bits.raw(1, 1); // uncompressed
        bits.align();
        for _ in 0..65536 {
            bits.raw(42, 8);
        }
    }
    bits.raw(3, 2); // final, empty
    bits.align();
    let mut packet = vec![1, 0, 0x88, 0x13, 1, 0]; // v1, 5000 ms, one field
    packet.extend_from_slice(&(bits.bytes.len() as u32).to_le_bytes());
    packet.extend_from_slice(&2u16.to_le_bytes());
    packet.extend_from_slice(b"br");
    packet.extend_from_slice(&bits.bytes);
    packet
}

struct Worker {
    started: mpsc::Receiver<()>,
    resume: mpsc::SyncSender<()>,
    result: mpsc::Receiver<Vec<u8>>,
    handle: Option<JoinHandle<()>>,
}
impl Worker {
    fn start(inbox: Arc<Inbox>) -> Self {
        let packet = packet();
        let (started, ready) = mpsc::sync_channel(1);
        let (resume, next) = mpsc::sync_channel(1);
        let (result, answer) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let mut calls = 0;
            let reply = adapter::execute(&packet, &mut || {
                calls += 1;
                if calls == 20 {
                    started.send(()).unwrap();
                    next.recv_timeout(WAIT)
                        .expect("resume missing at inner-work checkpoint");
                }
                if inbox.is_interrupted(ACTIVE, adapter::OPERATION) {
                    Err(Abort::Cancelled)
                } else {
                    Ok(())
                }
            })
            .unwrap();
            result.send(reply).unwrap();
        });
        let worker = Self {
            started: ready,
            resume,
            result: answer,
            handle: Some(handle),
        };
        worker
            .started
            .recv_timeout(WAIT)
            .expect("native inner-work checkpoint not reached");
        worker
    }
    fn finish(mut self, interrupted: bool) {
        self.resume.send(()).unwrap();
        let reply = self
            .result
            .recv_timeout(WAIT)
            .expect("native decode did not finish");
        self.handle.take().unwrap().join().unwrap();
        if interrupted {
            // Only typed metadata escapes; partial output bytes are withheld.
            assert_eq!(reply.len(), 22);
            assert_eq!(&reply[..3], &[1, 0, 1]);
            assert_eq!(
                u16::from_le_bytes(reply[3..5].try_into().unwrap()),
                adapter::Failure::Cancelled as u16
            );
            let input = u64::from_le_bytes(reply[6..14].try_into().unwrap());
            let output = u64::from_le_bytes(reply[14..22].try_into().unwrap());
            assert!(input > 0);
            assert!(
                output > 0 && output < PLAIN_SIZE as u64,
                "must abort partial decoding, not startup"
            );
        } else {
            assert_eq!(&reply[..3], &[1, 0, 0]);
            assert_eq!(reply[11], 1);
            assert_eq!(reply.len(), PLAIN_SIZE + 33);
            assert!(reply[33..].iter().all(|&value| value == 42));
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            let _ = self.resume.try_send(());
            let _ = handle.join();
        }
    }
}

enum ReadEvent {
    Accepted(u8, u64),
    Closed,
    Failed,
}
struct Ingress {
    socket: TcpStream,
    events: mpsc::Receiver<ReadEvent>,
    handle: Option<JoinHandle<()>>,
}
impl Ingress {
    fn start(inbox: Arc<Inbox>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let socket = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        socket.set_nodelay(true).unwrap();
        socket.set_write_timeout(Some(WAIT)).unwrap();
        let (mut read, _) = listener.accept().unwrap();
        read.set_read_timeout(Some(WAIT)).unwrap();
        let (send, events) = mpsc::channel();
        let handle = thread::spawn(move || {
            loop {
                // Same Frame reader + Inbox ingress contract as the real host.
                match bridge::Frame::read(&mut read) {
                    Ok(Some(frame)) => {
                        let key = (frame.kind, frame.id);
                        if !inbox.push(frame) {
                            send.send(ReadEvent::Failed).unwrap();
                            break;
                        }
                        send.send(ReadEvent::Accepted(key.0, key.1)).unwrap();
                    }
                    Ok(None) => {
                        inbox.close(None);
                        send.send(ReadEvent::Closed).unwrap();
                        break;
                    }
                    Err(error) => {
                        inbox.close(Some(error));
                        send.send(ReadEvent::Failed).unwrap();
                        break;
                    }
                }
            }
        });
        Self {
            socket,
            events,
            handle: Some(handle),
        }
    }
    fn send(&mut self, frame: bridge::Frame) {
        frame.write(&mut self.socket).unwrap();
        assert!(
            matches!(self.events.recv_timeout(WAIT).unwrap(), ReadEvent::Accepted(kind, id) if kind == frame.kind && id == frame.id)
        );
    }
    fn fill(&mut self) {
        for id in 1..=QUEUE as u64 {
            self.send(request(ACTIVE + id));
        }
    }
    fn eof(&mut self) {
        self.socket.shutdown(Shutdown::Write).unwrap();
        assert!(matches!(
            self.events.recv_timeout(WAIT).unwrap(),
            ReadEvent::Closed
        ));
    }
    fn invalid(&mut self, bytes: &[u8]) {
        self.socket.write_all(bytes).unwrap();
        self.socket.shutdown(Shutdown::Write).unwrap();
        assert!(matches!(
            self.events.recv_timeout(WAIT).unwrap(),
            ReadEvent::Failed
        ));
    }
}
impl Drop for Ingress {
    fn drop(&mut self) {
        let _ = self.socket.shutdown(Shutdown::Both);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn setup() -> (Arc<Inbox>, Worker, Ingress) {
    let inbox = Arc::new(Inbox::default());
    // Remove the real active request exactly as synchronous host dispatch does.
    assert!(inbox.push(request(ACTIVE)));
    assert!(matches!(inbox.receive(Duration::ZERO), Event::Frame(f) if f.id == ACTIVE));
    let worker = Worker::start(Arc::clone(&inbox));
    let ingress = Ingress::start(Arc::clone(&inbox));
    (inbox, worker, ingress)
}

#[test]
fn in_flight_cancel_is_read_while_request_queue_is_saturated() {
    let (inbox, worker, mut ingress) = setup();
    ingress.fill();
    ingress.send(cancel(ACTIVE, adapter::OPERATION));
    assert!(inbox.is_interrupted(ACTIVE, adapter::OPERATION));
    worker.finish(true);
    assert!(
        matches!(inbox.receive(Duration::ZERO), Event::Frame(f) if f.kind == bridge::CANCEL && f.id == ACTIVE)
    );
    for id in 1..=QUEUE as u64 {
        assert!(matches!(inbox.receive(Duration::ZERO), Event::Frame(f) if f.id == ACTIVE + id));
    }
    assert!(matches!(inbox.receive(Duration::ZERO), Event::Timeout));
}

#[test]
fn queued_cancel_removes_work_and_leaves_active_and_siblings_usable() {
    let (inbox, worker, mut ingress) = setup();
    ingress.fill();
    ingress.send(cancel(ACTIVE + 128, adapter::OPERATION));
    ingress.send(cancel(ACTIVE + 128, adapter::OPERATION)); // bounded deduplication
    ingress.send(request(ACTIVE + 257)); // proves the queue slot was recovered
    assert!(!inbox.is_interrupted(ACTIVE, adapter::OPERATION));
    worker.finish(false);
    assert!(
        matches!(inbox.receive(Duration::ZERO), Event::Frame(f) if f.kind == bridge::CANCEL && f.id == ACTIVE + 128)
    );
    let mut dispatched = Vec::new();
    while let Event::Frame(f) = inbox.receive(Duration::ZERO) {
        dispatched.push(f.id);
    }
    assert_eq!(dispatched.len(), QUEUE);
    assert!(!dispatched.contains(&(ACTIVE + 128)));
    assert_eq!(dispatched.last(), Some(&(ACTIVE + 257)));
}

#[test]
fn cancellation_query_pairs_request_id_with_operation() {
    let (inbox, worker, mut ingress) = setup();
    ingress.send(cancel(ACTIVE, adapter::OPERATION + 1));
    ingress.send(cancel(ACTIVE + 1, adapter::OPERATION));
    assert!(!inbox.is_interrupted(ACTIVE, adapter::OPERATION));
    assert!(inbox.is_interrupted(ACTIVE, adapter::OPERATION + 1));
    assert!(inbox.is_interrupted(ACTIVE + 1, adapter::OPERATION));
    worker.finish(false);
}

#[test]
fn in_flight_eof_clears_a_saturated_queue_and_withholds_partial_body() {
    let (inbox, worker, mut ingress) = setup();
    ingress.fill();
    ingress.eof();
    assert!(inbox.is_interrupted(ACTIVE, adapter::OPERATION));
    worker.finish(true);
    assert!(matches!(inbox.receive(Duration::ZERO), Event::Closed(None)));
    assert!(!inbox.push(request(ACTIVE + 257)));
}

#[test]
fn in_flight_protocol_failure_withholds_partial_body_and_discards_queue() {
    for truncated in [false, true] {
        let (inbox, worker, mut ingress) = setup();
        ingress.fill();
        ingress.invalid(if truncated { &[16, 0] } else { &[1, 0, 0, 0] });
        worker.finish(true);
        assert!(matches!(
            inbox.receive(Duration::ZERO),
            Event::Closed(Some(_))
        ));
        assert!(!inbox.push(request(ACTIVE + 257)));
    }
}

#[test]
fn in_flight_request_overload_closes_ingress_and_aborts_partial_decode() {
    let (inbox, worker, mut ingress) = setup();
    ingress.fill();
    request(ACTIVE + 257).write(&mut ingress.socket).unwrap();
    assert!(matches!(
        ingress.events.recv_timeout(WAIT).unwrap(),
        ReadEvent::Failed
    ));
    worker.finish(true);
    let Event::Closed(Some(failure)) = inbox.receive(Duration::ZERO) else {
        panic!("typed overload expected");
    };
    assert_eq!(
        failure.reply,
        Some(request(ACTIVE + 257).error(4, "application request queue limit exceeded"))
    );
    assert!(!inbox.push(request(ACTIVE + 258)));
}

#[test]
fn in_flight_control_overload_closes_ingress_and_aborts_partial_decode() {
    let (inbox, worker, mut ingress) = setup();
    for id in 1..=QUEUE as u64 {
        ingress.send(cancel(ACTIVE + id, adapter::OPERATION));
    }
    cancel(ACTIVE + 257, adapter::OPERATION)
        .write(&mut ingress.socket)
        .unwrap();
    assert!(matches!(
        ingress.events.recv_timeout(WAIT).unwrap(),
        ReadEvent::Failed
    ));
    worker.finish(true);
    assert!(matches!(
        inbox.receive(Duration::ZERO),
        Event::Closed(Some(_))
    ));
    assert!(!inbox.push(request(ACTIVE + 258)));
}
