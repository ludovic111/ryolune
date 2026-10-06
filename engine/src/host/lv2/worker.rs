//! `worker:schedule` (LV2 Worker 1.2): a plugin asks for non-realtime work from `run()`; a
//! thread of the instance does it and the answers come back to the audio thread before the
//! next `run()` ends (`work_response`, then `end_run`). Requests and responses travel through
//! wait-free byte rings, length-prefixed; the audio side never allocates or locks.

use super::ffi::{self, LV2_Handle, LV2_Worker_Interface};
use std::{
    cell::UnsafeCell,
    ffi::c_void,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

const RING: usize = 1 << 18;
/// The largest single message; bigger ones are refused with `LV2_WORKER_ERR_NO_SPACE`.
pub const MAX_MESSAGE: usize = 1 << 16;

/// What `schedule_work` writes into. Its address is the schedule handle the plugin keeps.
pub struct Requests {
    producer: UnsafeCell<rtrb::Producer<u8>>,
    /// `schedule_work` is called from `run()` and, rarely, from the main thread during
    /// `instantiate` or `restore`; those never overlap, and this flag makes sure of it.
    busy: AtomicBool,
    thread: std::sync::OnceLock<std::thread::Thread>,
}
// SAFETY: the producer is only touched by whoever holds `busy`.
unsafe impl Send for Requests {}
unsafe impl Sync for Requests {}

fn push(producer: &mut rtrb::Producer<u8>, data: &[u8]) -> bool {
    if data.len() > MAX_MESSAGE || producer.slots() < data.len() + 4 {
        return false;
    }
    producer
        .push_entire_slice(&(data.len() as u32).to_ne_bytes())
        .is_ok()
        && producer.push_entire_slice(data).is_ok()
}
/// Read one length-prefixed message into `scratch`, returning its size.
fn pop(consumer: &mut rtrb::Consumer<u8>, scratch: &mut [u8]) -> Option<usize> {
    if consumer.slots() < 4 {
        return None;
    }
    let mut len = [0u8; 4];
    consumer.pop_entire_slice(&mut len).ok()?;
    let len = u32::from_ne_bytes(len) as usize;
    if len > scratch.len() {
        // Cannot happen: `push` refuses anything larger.
        for _ in 0..len {
            let _ = consumer.pop();
        }
        return None;
    }
    consumer.pop_entire_slice(&mut scratch[..len]).ok()?;
    Some(len)
}

unsafe extern "C" fn schedule_work(handle: *mut c_void, size: u32, data: *const c_void) -> u32 {
    let requests = &*(handle as *const Requests);
    if requests.busy.swap(true, Ordering::Acquire) {
        return ffi::LV2_WORKER_ERR_NO_SPACE;
    }
    let bytes: &[u8] = if size == 0 || data.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(data as *const u8, size as usize)
    };
    let ok = push(&mut *requests.producer.get(), bytes);
    requests.busy.store(false, Ordering::Release);
    if !ok {
        return ffi::LV2_WORKER_ERR_NO_SPACE;
    }
    if let Some(thread) = requests.thread.get() {
        thread.unpark();
    }
    ffi::LV2_WORKER_SUCCESS
}
unsafe extern "C" fn respond(handle: *mut c_void, size: u32, data: *const c_void) -> u32 {
    let producer = &mut *(handle as *mut rtrb::Producer<u8>);
    let bytes: &[u8] = if size == 0 || data.is_null() {
        &[]
    } else {
        std::slice::from_raw_parts(data as *const u8, size as usize)
    };
    if push(producer, bytes) {
        ffi::LV2_WORKER_SUCCESS
    } else {
        ffi::LV2_WORKER_ERR_NO_SPACE
    }
}

struct SendHandle(LV2_Handle, *const LV2_Worker_Interface);
// SAFETY: LV2 lets `work` run on its own thread, concurrently with `run`.
unsafe impl Send for SendHandle {}

/// The non-realtime half: the thread and the request ring's other end.
pub struct Worker {
    requests: Box<Requests>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Taken by the processor (audio thread) when it is built.
    responses: Option<rtrb::Consumer<u8>>,
    pending_consumer: Option<rtrb::Consumer<u8>>,
    pending_responder: Option<rtrb::Producer<u8>>,
}
impl Worker {
    /// The rings, before the instance exists (the schedule feature goes into `instantiate`).
    pub fn new() -> Self {
        let (producer, consumer) = rtrb::RingBuffer::new(RING);
        let (responder, responses) = rtrb::RingBuffer::new(RING);
        Self {
            requests: Box::new(Requests {
                producer: UnsafeCell::new(producer),
                busy: AtomicBool::new(false),
                thread: std::sync::OnceLock::new(),
            }),
            stop: Arc::new(AtomicBool::new(false)),
            thread: None,
            responses: Some(responses),
            pending_consumer: Some(consumer),
            pending_responder: Some(responder),
        }
    }
    pub fn schedule(&self) -> ffi::LV2_Worker_Schedule {
        ffi::LV2_Worker_Schedule {
            handle: &*self.requests as *const Requests as *mut c_void,
            schedule_work,
        }
    }
    /// Start the thread once the instance and its worker interface exist.
    pub fn start(&mut self, name: &str, handle: LV2_Handle, interface: *const LV2_Worker_Interface) {
        let (Some(mut consumer), Some(responder)) =
            (self.pending_consumer.take(), self.pending_responder.take())
        else {
            return;
        };
        let stop = self.stop.clone();
        let target = SendHandle(handle, interface);
        let spawned = std::thread::Builder::new()
            .name(format!("lv2 worker: {name}"))
            .spawn(move || {
                let target = target;
                let mut responder = Box::new(responder);
                let mut scratch = vec![0u8; MAX_MESSAGE];
                let Some(work) = (unsafe { (*target.1).work }) else {
                    return;
                };
                while !stop.load(Ordering::Acquire) {
                    match pop(&mut consumer, &mut scratch) {
                        Some(len) => unsafe {
                            work(
                                target.0,
                                respond,
                                &mut *responder as *mut rtrb::Producer<u8> as *mut c_void,
                                len as u32,
                                scratch.as_ptr() as *const c_void,
                            );
                        },
                        None => std::thread::park_timeout(std::time::Duration::from_millis(20)),
                    }
                }
            });
        if let Ok(thread) = spawned {
            let _ = self.requests.thread.set(thread.thread().clone());
            self.thread = Some(thread);
        }
    }
    pub fn take_responses(&mut self) -> Option<rtrb::Consumer<u8>> {
        self.responses.take()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// The audio thread's half: answers waiting for `work_response`.
pub struct Responses {
    consumer: rtrb::Consumer<u8>,
    scratch: Vec<u8>,
}
impl Responses {
    pub fn new(consumer: rtrb::Consumer<u8>) -> Self {
        Self {
            consumer,
            scratch: vec![0u8; MAX_MESSAGE],
        }
    }
    /// Deliver every waiting answer, then `end_run`. Audio thread, after `run()`.
    pub fn deliver(&mut self, handle: LV2_Handle, interface: *const LV2_Worker_Interface) {
        unsafe {
            let interface = &*interface;
            if let Some(work_response) = interface.work_response {
                while let Some(len) = pop(&mut self.consumer, &mut self.scratch) {
                    work_response(handle, len as u32, self.scratch.as_ptr() as *const c_void);
                }
            }
            if let Some(end_run) = interface.end_run {
                end_run(handle);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn messages_keep_their_bounds() {
        let (mut p, mut c) = rtrb::RingBuffer::new(64);
        assert!(push(&mut p, b"hello"));
        assert!(push(&mut p, b""));
        assert!(!push(&mut p, &[0u8; 100]), "too big for the ring");
        let mut scratch = [0u8; 16];
        assert_eq!(pop(&mut c, &mut scratch), Some(5));
        assert_eq!(&scratch[..5], b"hello");
        assert_eq!(pop(&mut c, &mut scratch), Some(0));
        assert_eq!(pop(&mut c, &mut scratch), None);
    }
}
