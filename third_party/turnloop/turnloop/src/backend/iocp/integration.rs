//! Opt-in, sole IOCP consumer. The host drains a bounded preallocated queue.
use super::port::{Entry, Port, STOP, Wait, bool_result, owned};
use crate::{Error, Result};
use std::{
    collections::VecDeque,
    io,
    os::windows::io::{AsRawHandle, OwnedHandle},
    ptr,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::Threading::{CreateEventW, SetEvent},
};

struct Queue {
    entries: VecDeque<Entry>,
    high_water: usize,
    stopping: bool,
    error: Option<Error>,
}
struct Shared {
    queue: Mutex<Queue>,
    room: Condvar,
    event: OwnedHandle,
    #[cfg(test)]
    wait_error: std::sync::atomic::AtomicI32,
}

pub struct EventIntegration {
    shared: Arc<Shared>,
    port: Arc<Port>,
    worker: Option<JoinHandle<()>>,
}

impl EventIntegration {
    /// The caller must stop calling Port::wait while this helper is active.
    pub fn new(port: Arc<Port>) -> io::Result<Self> {
        // SAFETY: auto-reset event, initially nonsignaled, owned until helper joined.
        let event = unsafe { owned(CreateEventW(ptr::null(), 0, 0, ptr::null())) }?;
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue {
                entries: VecDeque::with_capacity(128),
                high_water: 0,
                stopping: false,
                error: None,
            }),
            room: Condvar::new(),
            event,
            #[cfg(test)]
            wait_error: std::sync::atomic::AtomicI32::new(0),
        });
        let worker_shared = Arc::clone(&shared);
        let worker_port = Arc::clone(&port);
        let worker = std::thread::Builder::new()
            .name("turnloop-iocp-event".into())
            .spawn(move || {
                if let Err(error) = pump(&worker_shared, &worker_port) {
                    let mut queue = worker_shared
                        .queue
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    queue.error = Some(error.into());
                    worker_shared.room.notify_all();
                    // SAFETY: event owned by worker_shared; report pump failure to host.
                    unsafe {
                        SetEvent(worker_shared.event.as_raw_handle());
                    }
                }
            })?;
        Ok(Self {
            shared,
            port,
            worker: Some(worker),
        })
    }
    pub fn event(&self) -> HANDLE {
        self.shared.event.as_raw_handle()
    }
    #[cfg(test)]
    pub(super) fn fail_and_wait(&self, code: i32) {
        use std::sync::atomic::Ordering;
        self.shared.wait_error.store(code, Ordering::Release);
        self.port
            .post(super::port::WAKE, 0)
            .expect("wake pump for fault");
        let queue = self.shared.queue.lock().expect("fault queue");
        let (queue, timeout) = self
            .shared
            .room
            .wait_timeout_while(queue, std::time::Duration::from_secs(5), |q| {
                q.error.is_none()
            })
            .expect("fault publication");
        assert!(!timeout.timed_out(), "pump did not run injected failure");
        assert_eq!(queue.error.expect("published error").os, Some(code));
        assert_eq!(self.shared.wait_error.load(Ordering::Acquire), 0);
    }
    pub fn check(&self) -> Result<()> {
        let queue = self
            .shared
            .queue
            .lock()
            .map_err(|_| io::Error::other("helper queue poisoned"))?;
        if let Some(error) = queue.error {
            // SAFETY: keep the host's auto-reset event observable after every
            // failed turn. Retain the original error even if signaling fails.
            unsafe {
                SetEvent(self.event());
            }
            return Err(error);
        }
        Ok(())
    }
    pub fn drain(&mut self, out: &mut [Entry]) -> Result<usize> {
        self.check()?;
        self.drain_retained(out)
    }
    /// Also used after shutdown to recover packets dequeued before pump failure.
    pub fn drain_retained(&mut self, out: &mut [Entry]) -> Result<usize> {
        let mut queue = self
            .shared
            .queue
            .lock()
            .map_err(|_| io::Error::other("helper queue poisoned"))?;
        let mut n = 0;
        for slot in out {
            let Some(entry) = queue.entries.pop_front() else {
                break;
            };
            *slot = entry;
            n += 1;
        }
        if !queue.entries.is_empty() {
            // SAFETY: live auto-reset event; partial drains must re-signal while
            // holding the queue lock, or the external waiter can miss remaining work.
            // https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-setevent
            bool_result(unsafe { SetEvent(self.event()) })?;
        }
        self.shared.room.notify_one();
        Ok(n)
    }
    pub fn shutdown(&mut self) -> io::Result<()> {
        if self.worker.is_none() {
            return Ok(());
        }
        {
            let mut queue = self.shared.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.stopping = true;
            self.shared.room.notify_all();
        }
        self.port.post(STOP, 0)?;
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| io::Error::other("helper panicked"))?;
        }
        Ok(())
    }
}
impl Drop for EventIntegration {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn pump(shared: &Shared, port: &Port) -> io::Result<()> {
    let mut batch = [Entry::default(); 64];
    loop {
        {
            let mut queue = shared
                .queue
                .lock()
                .map_err(|_| io::Error::other("helper queue poisoned"))?;
            // Reserve space for a whole OS batch before removing it from the
            // port. Shutdown and wake failures must never discard a partial batch.
            while queue.entries.len() > 128 - batch.len() && !queue.stopping {
                queue = shared
                    .room
                    .wait(queue)
                    .map_err(|_| io::Error::other("helper queue poisoned"))?;
            }
            if queue.stopping {
                return Ok(());
            }
        }
        #[cfg(test)]
        {
            let code = shared
                .wait_error
                .swap(0, std::sync::atomic::Ordering::AcqRel);
            if code != 0 {
                return Err(io::Error::from_raw_os_error(code));
            }
        }
        let Wait::Entries(n) = port.wait(None, false, &mut batch)? else {
            continue;
        };
        let mut queue = shared
            .queue
            .lock()
            .map_err(|_| io::Error::other("helper queue poisoned"))?;
        for entry in &batch[..n] {
            if entry.key == STOP {
                queue.stopping = true;
            } else {
                queue.entries.push_back(*entry); // full batch space reserved above
            }
        }
        queue.high_water = queue.high_water.max(queue.entries.len());
        #[cfg(test)]
        shared.room.notify_all();
        // SAFETY: shared owns live event; entire batch retained before wake.
        bool_result(unsafe { SetEvent(shared.event.as_raw_handle()) })?;
        if queue.stopping {
            return Ok(());
        }
    }
}

#[cfg(all(test, not(loom)))]
mod tests {
    use super::*;

    #[test]
    fn shutdown_preserves_every_packet_under_full_queue_backpressure() {
        let port = Arc::new(Port::new().expect("port"));
        // Queue before starting the sole consumer, giving it two complete batches.
        for i in 0..128 {
            port.post(17, i).expect("post subject packet");
        }
        let mut helper = EventIntegration::new(Arc::clone(&port)).expect("helper");
        {
            let queue = helper.shared.queue.lock().expect("queue");
            let (queue, timeout) = helper
                .shared
                .room
                .wait_timeout_while(queue, std::time::Duration::from_secs(5), |q| {
                    q.high_water != 128
                })
                .expect("pump filled queue");
            assert!(!timeout.timed_out(), "pump never reached capacity");
            assert_eq!(queue.entries.len(), 128);
        }
        helper.shutdown().expect("join backpressured worker");
        let mut batch = [Entry::default(); 7]; // force partial retained drains
        let mut seen = [false; 128];
        let mut count = 0;
        loop {
            let n = helper.drain_retained(&mut batch).expect("retained batch");
            if n == 0 {
                break;
            }
            for entry in &batch[..n] {
                assert_eq!(entry.key, 17);
                assert!(!seen[entry.bytes as usize]);
                seen[entry.bytes as usize] = true;
                count += 1;
            }
        }
        assert_eq!(count, 128);
        assert!(seen.into_iter().all(|seen| seen));
    }
}
