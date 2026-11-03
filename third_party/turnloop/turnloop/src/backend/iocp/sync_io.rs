//! One worker per direction of an adopted synchronous handle. Requests retain their caller
//! buffers until the worker's completion packet; cancellation joins buffer access.
//!
//! Windows serializes every I/O request on a synchronous file object: an idle ReadFile
//! holds the object's I/O lock inside the kernel, so WriteFile (and control I/O such as
//! GetConsoleMode) on the same object waits until the read completes. Both workers use
//! one object, so for synchronous pipes the write worker preempts an idle read with
//! CancelSynchronousIo before writing. A cancelled pipe read has consumed no bytes (named
//! pipe reads complete only with data), and the read worker reissues the same request
//! after the write, so read FIFO order and exactly-once completion are unchanged. A write
//! that waits for the peer to drain its buffer still holds the lock; reads on that
//! endpoint queue behind it until it completes or is cancelled. libuv's non-overlapped
//! pipes have the same kernel constraint (see `uv_pipe_getsockname` in pipe.c).
use super::{
    Detached, Kind, Native, bool_result, os_error, port::Port, process, signals, unsupported,
};
use crate::{Result, Signal};
use std::{
    os::windows::io::{AsRawHandle, OwnedHandle},
    ptr,
    sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, mpsc},
    thread::JoinHandle,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::*,
    Storage::FileSystem::{FILE_TYPE_PIPE, GetFileType, ReadFile, WriteFile},
    System::{
        Console::*,
        IO::{CancelSynchronousIo, IO_STATUS_BLOCK, OVERLAPPED, PostQueuedCompletionStatus},
        Threading::Sleep,
    },
};
pub(super) const KEY: usize = 3;
const READ: usize = 0;
const WRITE: usize = 1;
#[derive(Clone, Copy)]
struct Job {
    pointer: usize,
    len: u32,
    overlapped: usize,
}
#[derive(Default)]
struct Direction {
    job: Option<Job>,
    active: bool,
    // Set under the lock immediately before the worker's native I/O call and cleared
    // under the lock after it returns. Cancellation outside this interval needs no
    // CancelSynchronousIo: the worker rechecks `cancelled` before entering again.
    in_io: bool,
    stop: bool,
    cancelled: bool,
    result: Option<Result<u32>>,
}
struct State {
    directions: [Direction; 2],
    // Duplicated worker thread handles, registered before any request can start.
    threads: [Option<OwnedHandle>; 2],
    // A pipe write owns the file object: the read worker must not enter ReadFile.
    writing: bool,
    // The write worker is cancelling the read worker's idle ReadFile.
    preempt: bool,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    port: Arc<Port>,
    handle: OwnedHandle,
    console: bool,
    preemptible: bool,
}
/// A worker's shared state and direction, sent to the process-wide cancellation helper.
type Cancellations = mpsc::Sender<(Arc<Shared>, usize)>;
pub(super) struct Worker {
    shared: Arc<Shared>,
    direction: usize,
    thread: Option<JoinHandle<()>>,
    canceller: Cancellations,
}

fn lock(shared: &Shared) -> MutexGuard<'_, State> {
    shared.state.lock().unwrap_or_else(|e| e.into_inner())
}
fn cancel_thread(state: &State, direction: usize) -> bool {
    let thread = state.threads[direction]
        .as_ref()
        .expect("worker thread registered before requests");
    // SAFETY: the duplicated handle pins the thread identity. Callers hold the state
    // lock while `in_io` is set, so this cannot reach a later request of the worker.
    if unsafe { CancelSynchronousIo(thread.as_raw_handle()) } != 0 {
        return true;
    }
    // SAFETY: reads this thread's last error immediately after the failed call.
    if unsafe { GetLastError() } != ERROR_NOT_FOUND {
        std::process::abort();
    }
    false
}
fn readable_and_writable(handle: HANDLE) -> bool {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_ACCESS_INFORMATION, FileAccessInformation, NtQueryInformationFile,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_READ_DATA, FILE_WRITE_DATA};
    // SAFETY: valid zeroed C output storage.
    let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    let mut info = FILE_ACCESS_INFORMATION { AccessFlags: 0 };
    // SAFETY: live quiescent handle and correctly sized writable output; the query
    // retains no pointer. libuv's uv_pipe_open makes the same query at adoption.
    let result = unsafe {
        NtQueryInformationFile(
            handle,
            &mut status,
            ptr::from_mut(&mut info).cast(),
            std::mem::size_of_val(&info) as u32,
            FileAccessInformation,
        )
    };
    let both = FILE_READ_DATA | FILE_WRITE_DATA;
    // An unknown answer keeps preemption: it is only a cost when a write actually
    // arrives beside an idle read.
    result != 0 || info.AccessFlags & both == both
}
fn aborted(result: &Result<u32>) -> bool {
    matches!(result, Err(error) if error.os == Some(ERROR_OPERATION_ABORTED as i32))
}

// Repeats CancelSynchronousIo until the worker in `direction` leaves native I/O or
// `keep` becomes false; the lock is held on entry and on return. Two races need the
// retry. Before kernel entry the call fails with ERROR_NOT_FOUND; the worker is
// runnable, so yield (sleeping 1 ms only after 64 misses, which requires a foreign
// holder of the file object's I/O lock). Just after entry the call can report success
// yet be lost: windows-2025 run 34932207539 showed a named-pipe ReadFile staying
// pending after a successful cancellation until a later call. So a success waits at
// most 10 ms for the worker's wakeup before cancelling again. Neither retry runs for
// idle or completed I/O, and nothing polls ordinary progress.
fn cancel_until_returned<'a>(
    shared: &'a Shared,
    mut state: MutexGuard<'a, State>,
    direction: usize,
    keep: impl Fn(&State) -> bool,
) -> MutexGuard<'a, State> {
    let mut misses = 0u32;
    while state.directions[direction].in_io && keep(&state) {
        if cancel_thread(&state, direction) {
            state = shared
                .changed
                .wait_timeout(state, Duration::from_millis(10))
                .unwrap_or_else(|e| e.into_inner())
                .0;
            continue;
        }
        drop(state);
        if misses < 64 {
            std::thread::yield_now();
        } else {
            // SAFETY: plain millisecond sleep with no pointers or handles.
            unsafe { Sleep(1) };
        }
        misses += 1;
        state = lock(shared);
    }
    state
}

// One process-wide helper completes user cancellations without blocking a turn or
// creating a per-request thread. It sleeps on its queue while idle.
fn canceller() -> Result<Cancellations> {
    static CANCELLER: OnceLock<Result<Cancellations>> = OnceLock::new();
    CANCELLER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::channel::<(Arc<Shared>, usize)>();
            std::thread::Builder::new()
                .name("turnloop-stdio-cancel".into())
                .spawn(move || {
                    while let Ok((shared, direction)) = receiver.recv() {
                        drop(cancel_until_returned(
                            &shared,
                            lock(&shared),
                            direction,
                            |state| state.directions[direction].cancelled,
                        ));
                    }
                })?;
            Ok(sender)
        })
        .clone()
}
impl Worker {
    /// Spawn the read (index 0) and write (index 1) workers for one adopted handle.
    pub(super) fn pair(handle: HANDLE, console: bool, port: Arc<Port>) -> Result<[Self; 2]> {
        let canceller = canceller()?;
        // Classified while quiescent. Console input and screen output are distinct
        // objects and disk reads do not wait for data, so only pipes need preemption.
        // Other character devices keep their driver's serialization semantics. A
        // one-way pipe end (every anonymous pipe) fails the other direction's access
        // check before any I/O serialization, so it needs no preemption either.
        // SAFETY: live adopted handle; GetFileType has no lifetime side effects.
        let preemptible = !console
            && unsafe { GetFileType(handle) } == FILE_TYPE_PIPE
            && readable_and_writable(handle);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                directions: [Direction::default(), Direction::default()],
                threads: [None, None],
                writing: false,
                preempt: false,
            }),
            changed: Condvar::new(),
            port,
            handle: process::duplicate(handle, false)?,
            console,
            preemptible,
        });
        let spawn = |direction: usize| -> Result<Self> {
            let run = Arc::clone(&shared);
            let thread = std::thread::Builder::new()
                .name("turnloop-stdio".into())
                .spawn(move || pump(run, direction))?;
            let worker = Self {
                shared: Arc::clone(&shared),
                direction,
                thread: Some(thread),
                canceller: canceller.clone(),
            };
            let duplicate = process::duplicate(
                worker.thread.as_ref().expect("worker").as_raw_handle(),
                false,
            )?;
            lock(&shared).threads[direction] = Some(duplicate);
            Ok(worker)
        };
        let read = spawn(READ)?;
        Ok([read, spawn(WRITE)?])
    }
    pub(super) fn start(&self, pointer: *mut u8, len: u32, overlapped: *mut OVERLAPPED) {
        let mut state = lock(&self.shared);
        let direction = &mut state.directions[self.direction];
        assert!(
            direction.job.is_none()
                && !direction.active
                && !direction.in_io
                && direction.result.is_none()
        );
        direction.cancelled = false;
        direction.job = Some(Job {
            pointer: pointer as usize,
            len,
            overlapped: overlapped as usize,
        });
        self.shared.changed.notify_all();
    }
    pub(super) fn cancel(&self) {
        let mut state = lock(&self.shared);
        state.directions[self.direction].cancelled = true;
        if state.directions[self.direction].in_io {
            // Cancellation does not acknowledge caller buffers; only the worker's
            // packet does. A missed or lost cancellation is retried by the helper.
            cancel_thread(&state, self.direction);
            if self
                .canceller
                .send((Arc::clone(&self.shared), self.direction))
                .is_err()
            {
                std::process::abort();
            }
        }
        // Wakes a worker waiting to start or a read parked behind a pipe write.
        self.shared.changed.notify_all();
    }
    pub(super) fn finish(&self) -> Result<u32> {
        lock(&self.shared).directions[self.direction]
            .result
            .take()
            .expect("worker completion")
    }
}

// Called by the write worker, holding the lock with `writing` already set so the read
// worker cannot enter ReadFile again. Returns once no read is inside native I/O, or
// when this write is cancelled (a still-pending preemption is then acknowledged by the
// read worker, which reissues its request because `writing` is cleared).
fn preempt_read<'a>(shared: &'a Shared, mut state: MutexGuard<'a, State>) -> MutexGuard<'a, State> {
    if state.directions[READ].in_io {
        state.preempt = true;
    }
    cancel_until_returned(shared, state, READ, |state| {
        !state.directions[WRITE].cancelled
    })
}
fn pump(shared: Arc<Shared>, direction: usize) {
    let mut console = ConsoleBytes::default();
    let write = direction == WRITE;
    let mut state = lock(&shared);
    loop {
        while state.directions[direction].job.is_none() && !state.directions[direction].stop {
            state = shared
                .changed
                .wait(state)
                .unwrap_or_else(|e| e.into_inner());
        }
        if state.directions[direction].stop {
            return;
        }
        let job = state.directions[direction].job.take().expect("job");
        state.directions[direction].active = true;
        let result = loop {
            if state.directions[direction].cancelled {
                break Err(super::socket::error(ERROR_OPERATION_ABORTED as i32));
            }
            if shared.preemptible {
                if write {
                    state.writing = true;
                    state = preempt_read(&shared, state);
                    if state.directions[WRITE].cancelled {
                        continue;
                    }
                } else if state.writing {
                    state = shared
                        .changed
                        .wait(state)
                        .unwrap_or_else(|e| e.into_inner());
                    continue;
                }
            }
            state.directions[direction].in_io = true;
            drop(state);
            let result = io(&shared, &mut console, write, job);
            state = lock(&shared);
            state.directions[direction].in_io = false;
            if shared.preemptible && !write {
                // The write worker may be waiting for this read to leave the kernel.
                shared.changed.notify_all();
                if std::mem::take(&mut state.preempt)
                    && aborted(&result)
                    && !state.directions[READ].cancelled
                {
                    // Preempted by a write before any byte arrived: reissue the same
                    // request, in place and still first in the read FIFO, after it.
                    continue;
                }
            }
            break result;
        };
        if write && shared.preemptible {
            state.writing = false;
        }
        let entry = &mut state.directions[direction];
        entry.active = false;
        entry.result = Some(result);
        // SAFETY: retained port and opaque stable kernel slot. Publication happens
        // under the same lock used by finish, after all native buffer access ends.
        if unsafe {
            PostQueuedCompletionStatus(
                shared.port.raw(),
                0,
                KEY,
                job.overlapped as *const OVERLAPPED,
            )
        } == 0
        {
            std::process::abort();
        }
        shared.changed.notify_all();
    }
}
fn io(shared: &Shared, console: &mut ConsoleBytes, write: bool, job: Job) -> Result<u32> {
    // Classification belongs to quiescent adoption. GetConsoleMode is itself I/O on
    // the shared file object and would serialize behind the other worker's request.
    // Console identity is immutable; tty_set_mode only changes its flags.
    if shared.console && !write {
        return console.read(
            shared.handle.as_raw_handle(),
            job.pointer as *mut u8,
            job.len,
        );
    }
    let mut n = 0;
    // SAFETY: backend retains exclusive read/immutable write memory until the packet
    // for this job is consumed. Handle is synchronous and owned.
    let ok = unsafe {
        if write {
            WriteFile(
                shared.handle.as_raw_handle(),
                job.pointer as *const u8,
                job.len,
                &mut n,
                ptr::null_mut(),
            )
        } else {
            ReadFile(
                shared.handle.as_raw_handle(),
                job.pointer as *mut u8,
                job.len,
                &mut n,
                ptr::null_mut(),
            )
        }
    };
    if ok == 0 { Err(os_error()) } else { Ok(n) }
}
impl Drop for Worker {
    fn drop(&mut self) {
        // Backend Drop first waits for completion acknowledgements. There is no
        // active request here, including after normal close and quiescent detach.
        {
            let mut state = lock(&self.shared);
            let direction = &mut state.directions[self.direction];
            assert!(!direction.active && direction.job.is_none());
            direction.stop = true;
            self.shared.changed.notify_all();
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            std::process::abort();
        }
    }
}
#[derive(Default)]
struct ConsoleBytes {
    bytes: [u8; 4],
    len: usize,
    offset: usize,
    repeats: u16,
    high: Option<u16>,
}
impl ConsoleBytes {
    fn read(&mut self, handle: HANDLE, pointer: *mut u8, len: u32) -> Result<u32> {
        if len == 0 {
            return Ok(0);
        }
        loop {
            if self.repeats > 0 {
                let n = (self.len - self.offset).min(len as usize);
                // SAFETY: worker retains exclusive caller buffer; source range initialized.
                unsafe {
                    ptr::copy_nonoverlapping(self.bytes.as_ptr().add(self.offset), pointer, n);
                }
                self.offset += n;
                if self.offset == self.len {
                    self.offset = 0;
                    self.repeats -= 1;
                }
                return Ok(n as u32);
            }
            // SAFETY: plain C console input output record.
            let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
            let mut read = 0;
            // SAFETY: owned console and one writable input record; synchronous worker only.
            bool_result(unsafe { ReadConsoleInputW(handle, &mut record, 1, &mut read) })?;
            if read == 0 {
                continue;
            }
            if record.EventType == WINDOW_BUFFER_SIZE_EVENT as u16 {
                signals::dispatch(Signal::WinCh);
                continue;
            }
            if record.EventType != KEY_EVENT as u16 {
                continue;
            }
            // SAFETY: EventType selected the initialized key-event union arm.
            let key = unsafe { record.Event.KeyEvent };
            if key.bKeyDown == 0 {
                continue;
            }
            // SAFETY: ReadConsoleInputW initializes the Unicode member.
            let unit = unsafe { key.uChar.UnicodeChar };
            if unit == 0 {
                continue;
            }
            if (0xd800..=0xdbff).contains(&unit) {
                self.high = Some(unit);
                continue;
            }
            let code = if let Some(high) = self.high.take() {
                if (0xdc00..=0xdfff).contains(&unit) {
                    0x10000 + ((high as u32 - 0xd800) << 10) + (unit as u32 - 0xdc00)
                } else {
                    unit as u32
                }
            } else {
                unit as u32
            };
            let ch = char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER);
            self.len = ch.encode_utf8(&mut self.bytes).len();
            self.offset = 0;
            self.repeats = key.wRepeatCount.max(1);
        }
    }
}
impl Detached {
    /// Adopt a quiescent file, pipe or console handle. Synchronous handles retain
    /// their console classification and use per-direction workers on attachment.
    /// Arbitrary character devices retain their driver's I/O serialization and
    /// cancellation semantics. Overlapped pipes use event-routed I/O even with a prior IOCP
    /// association. Other overlapped handles return Unsupported. Captured console
    /// modes are restored on close or drop.
    pub fn from_handle(handle: OwnedHandle) -> Result<Self> {
        let mut mode = 0;
        // SAFETY: owned handle, writable mode output.
        let console = unsafe { GetConsoleMode(handle.as_raw_handle(), &mut mode) } != 0;
        // SAFETY: valid zeroed C output storage for screen-buffer classification.
        let mut info = unsafe { std::mem::zeroed() };
        // SAFETY: owned handle; failure distinguishes input from screen-buffer handles.
        let console_input = console
            && unsafe { GetConsoleScreenBufferInfo(handle.as_raw_handle(), &mut info) } == 0;
        let kind = if console {
            Kind::Sync
        } else {
            handle_kind(handle.as_raw_handle())?
        };
        let mut transport = Self::new(Native::Handle(handle), kind, kind == Kind::Pipe);
        transport.mode = console.then_some(mode);
        transport.console_input = console_input;
        Ok(transport)
    }
}

fn handle_kind(handle: HANDLE) -> Result<Kind> {
    use windows_sys::Wdk::Storage::FileSystem::{
        FILE_MODE_INFORMATION, FILE_SYNCHRONOUS_IO_ALERT, FILE_SYNCHRONOUS_IO_NONALERT,
        FileModeInformation, NtQueryInformationFile,
    };
    // SAFETY: valid zeroed C output storage; FileModeInformation completes inline
    // and does not initiate data I/O or retain either output pointer.
    let mut status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    let mut info = FILE_MODE_INFORMATION { Mode: 0 };
    // SAFETY: live owned handle and correctly sized writable mode information.
    // https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntqueryinformationfile
    let result = unsafe {
        NtQueryInformationFile(
            handle,
            &mut status,
            ptr::from_mut(&mut info).cast(),
            std::mem::size_of_val(&info) as u32,
            FileModeInformation,
        )
    };
    if result != 0 {
        return Err(unsupported());
    }
    if info.Mode & (FILE_SYNCHRONOUS_IO_ALERT | FILE_SYNCHRONOUS_IO_NONALERT) != 0 {
        Ok(Kind::Sync)
    // SAFETY: live owned handle; classification has no lifetime side effects.
    } else if unsafe { GetFileType(handle) } == FILE_TYPE_PIPE {
        Ok(Kind::Pipe)
    } else {
        // Never submit synchronous ReadFile with null OVERLAPPED on an unknown
        // asynchronous handle: a pending result would outlive worker buffers.
        Err(unsupported())
    }
}
