//! ReadDirectoryChangesW watches completed through the loop's own IOCP.
//!
//! Each watch owns a directory handle associated with the port under a unique,
//! never-reused key, plus pinned OVERLAPPED and notification storage. A request
//! is re-armed after each completion, so the kernel buffers changes while the host
//! holds leases. A watched file is served by its parent directory, filtered by name.
//! Storage is freed only after the kernel acknowledged the last request.
use super::{
    Detached, os_error,
    port::{Entry, Port},
};
use crate::slots::{Slots, page_reserve};
use crate::{
    backend::{Event, Operation, Outcome, Request},
    fs::watch::Ring,
    *,
};
use std::{
    cell::UnsafeCell,
    collections::VecDeque,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    ptr,
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, INVALID_HANDLE_VALUE, RtlNtStatusToDosError},
    Storage::FileSystem::*,
    System::IO::{CancelIoEx, OVERLAPPED},
};

/// Notification storage in DWORD-aligned words (64 KiB, the network-share limit).
const WORDS: usize = 64 * 1024 / 8;
const FILTER: u32 = FILE_NOTIFY_CHANGE_FILE_NAME
    | FILE_NOTIFY_CHANGE_DIR_NAME
    | FILE_NOTIFY_CHANGE_ATTRIBUTES
    | FILE_NOTIFY_CHANGE_SIZE
    | FILE_NOTIFY_CHANGE_LAST_WRITE
    | FILE_NOTIFY_CHANGE_LAST_ACCESS
    | FILE_NOTIFY_CHANGE_CREATION
    | FILE_NOTIFY_CHANGE_SECURITY;

struct Watch {
    handle: Handle,
    key: usize,
    directory: OwnedHandle,
    kernel: Box<UnsafeCell<OVERLAPPED>>,
    buffer: Box<UnsafeCell<[u64; WORDS]>>,
    recursive: bool,
    /// For a watched file: its UTF-16 name within the watched parent directory.
    file: Option<Box<[u16]>>,
    /// Final component of the watched path, as WTF-8.
    name: Box<[u8]>,
    op: Option<OpId>,
    armed: bool,
    cancelling: bool,
    queued: bool,
    ring: Ring,
}
impl Watch {
    fn arm(&mut self) -> Result<()> {
        debug_assert!(!self.armed);
        // SAFETY: the previous request was dequeued (or none was issued), so the
        // pinned OVERLAPPED is inactive and may be reset.
        unsafe { *self.kernel.get() = std::mem::zeroed() };
        // SAFETY: live overlapped directory handle; buffer and OVERLAPPED are
        // boxed, never move, and are freed only after this request's packet.
        let ok = unsafe {
            ReadDirectoryChangesW(
                self.directory.as_raw_handle(),
                self.buffer.get().cast(),
                (WORDS * 8) as u32,
                i32::from(self.recursive),
                FILTER,
                ptr::null_mut(),
                self.kernel.get(),
                None,
            )
        };
        if ok == 0 {
            return Err(os_error());
        }
        self.armed = true;
        Ok(())
    }
    fn cancel_native(&self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        // SAFETY: exact live request; its acknowledgement packet is still required.
        if unsafe { CancelIoEx(self.directory.as_raw_handle(), self.kernel.get()) } == 0 {
            let error = os_error();
            if error.os != Some(ERROR_NOT_FOUND as i32) {
                return Err(error);
            }
        }
        Ok(())
    }
    fn parse(&mut self, n: usize) {
        if n == 0 {
            // The kernel buffer overflowed and its contents were discarded.
            self.ring.lost();
            return;
        }
        // SAFETY: the completed request initialized `n` bytes of the pinned buffer,
        // which no kernel request accesses until the next arm.
        let bytes = unsafe { std::slice::from_raw_parts(self.buffer.get().cast::<u8>(), n) };
        let mut at = 0;
        loop {
            if at + 12 > bytes.len() {
                self.ring.lost();
                return;
            }
            let word = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("word"));
            let (next, action, length) = (word(at) as usize, word(at + 4), word(at + 8) as usize);
            if at + 12 + length > bytes.len() {
                self.ring.lost();
                return;
            }
            // SAFETY: records are DWORD-aligned in u64 storage, so the name is
            // u16-aligned; its bytes lie within the initialized region checked above.
            let units = unsafe {
                std::slice::from_raw_parts(bytes.as_ptr().add(at + 12).cast::<u16>(), length / 2)
            };
            let kind = if action == FILE_ACTION_MODIFIED {
                WatchKind::Change
            } else {
                WatchKind::Rename
            };
            match &self.file {
                Some(file) if same_name(units, file) => self.ring.push(kind, &self.name),
                Some(_) => {}
                None => self.ring.push_wide(kind, units),
            }
            if next == 0 {
                return;
            }
            at += next;
        }
    }
}
fn same_name(a: &[u16], b: &[u16]) -> bool {
    let fold = |c: u16| {
        if (u16::from(b'a')..=u16::from(b'z')).contains(&c) {
            c - 32
        } else {
            c
        }
    };
    a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| fold(x) == fold(y))
}

pub(super) struct Watches {
    entries: Slots<Watch>,
    active: Vec<usize>,
    /// Released watches whose last request awaits its acknowledgement.
    retired: Vec<Watch>,
    ops: Slots<Handle>,
    ready: VecDeque<Handle>,
    finished: VecDeque<(OpId, Result<()>)>,
    pool: BufferPool,
    port: Arc<Port>,
}
impl Watches {
    pub fn new(config: &Config, pool: BufferPool, port: Arc<Port>) -> Self {
        Self {
            entries: Slots::new(config.max_handles),
            active: Vec::with_capacity(page_reserve(config.max_handles)),
            retired: Vec::with_capacity(page_reserve(config.max_handles)),
            ops: Slots::new(config.max_operations),
            ready: VecDeque::with_capacity(page_reserve(config.max_handles)),
            finished: VecDeque::with_capacity(page_reserve(config.max_operations)),
            pool,
            port,
        }
    }
    fn get_mut(&mut self, h: Handle) -> Option<&mut Watch> {
        self.entries
            .get_mut(h.index())
            .and_then(Option::as_mut)
            .filter(|w| w.handle == h)
    }
    pub fn contains(&self, h: Handle) -> bool {
        self.entries
            .get(h.index())
            .and_then(Option::as_ref)
            .is_some_and(|w| w.handle == h)
    }
    pub fn start(&mut self, h: Handle, path: &FsPath, recursive: bool, key: usize) -> Result<()> {
        if self.entries.get(h.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let wide = &path.wide()[..path.wide().len() - 1];
        let end = wide
            .iter()
            .rposition(|&c| c != 0x5c && c != 0x2f)
            .map_or(0, |i| i + 1);
        let trimmed = &wide[..end];
        let split = trimmed.iter().rposition(|&c| c == 0x5c || c == 0x2f);
        let base = match split {
            Some(i) => &trimmed[i + 1..],
            None => trimmed,
        };
        let mut name = vec![0u8; base.len() * 3].into_boxed_slice();
        let mut used = 0;
        crate::fs::wtf8(base, &mut name, &mut used);
        let name: Box<[u8]> = name[..used].into();
        // SAFETY: NUL-terminated prepared path.
        let attributes = unsafe { GetFileAttributesW(path.wide().as_ptr()) };
        if attributes == INVALID_FILE_ATTRIBUTES {
            return Err(os_error());
        }
        let (directory, file) = if attributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
            (path.wide().to_vec(), None)
        } else {
            if recursive {
                return Err(Error::new(ErrorKind::Unsupported));
            }
            let mut parent = match split {
                Some(0) => trimmed[..1].to_vec(),
                Some(i) => trimmed[..i].to_vec(),
                None => vec![u16::from(b'.')],
            };
            parent.push(0);
            (parent, Some(base.into()))
        };
        // SAFETY: NUL-terminated directory path; the new handle is owned below.
        let raw = unsafe {
            CreateFileW(
                directory.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                ptr::null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(os_error());
        }
        // SAFETY: CreateFileW returned a new handle owned by nobody else.
        let directory = unsafe { OwnedHandle::from_raw_handle(raw) };
        // SAFETY: fresh, quiescent overlapped handle and a never-reused key.
        unsafe { self.port.associate(directory.as_raw_handle(), key) }?;
        let mut watch = Watch {
            handle: h,
            key,
            directory,
            // SAFETY: an all-zero OVERLAPPED is the inactive state.
            kernel: Box::new(UnsafeCell::new(unsafe { std::mem::zeroed() })),
            buffer: Box::new(UnsafeCell::new([0; WORDS])),
            recursive,
            file,
            name,
            op: None,
            armed: false,
            cancelling: false,
            queued: false,
            ring: Ring::new(),
        };
        watch.arm()?;
        self.entries[h.index()] = Some(watch);
        self.active.push(h.index());
        Ok(())
    }
    pub fn submit(&mut self, request: &Request) -> Result<()> {
        let op = request.op;
        if !matches!(request.operation, Operation::WatchFs)
            || self.ops.get(op.index()).is_none_or(Option::is_some)
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let watch = self
            .get_mut(request.handle)
            .filter(|w| w.op.is_none())
            .ok_or(Error::new(ErrorKind::InvalidInput))?;
        watch.op = Some(op);
        let queue = !watch.queued && !watch.ring.is_empty();
        watch.queued |= queue;
        self.ops[op.index()] = Some(request.handle);
        if queue {
            self.ready.push_back(request.handle);
        }
        Ok(())
    }
    /// Begin cancellation; the terminal result follows the kernel acknowledgement.
    pub fn cancel(&mut self, op: OpId) -> Result<bool> {
        let Some(h) = self.ops.get(op.index()).copied().flatten() else {
            return Ok(false);
        };
        let Some(watch) = self.get_mut(h).filter(|w| w.op == Some(op)) else {
            return Ok(false);
        };
        if watch.cancelling {
            return Ok(true);
        }
        watch.cancel_native()?;
        watch.cancelling = true;
        watch.ring.clear();
        if !watch.armed {
            self.finish(h);
        }
        Ok(true)
    }
    /// Queue the Cancelled terminal of a cancelling watch whose request is quiescent.
    fn finish(&mut self, h: Handle) {
        let Some(watch) = self.get_mut(h) else { return };
        debug_assert!(watch.cancelling && !watch.armed);
        if let Some(op) = watch.op.take() {
            self.ops[op.index()] = None;
            self.finished.push_back((op, Ok(())));
        }
    }
    /// Apply a port packet addressed to a watch key. Returns false for other keys.
    pub fn completed(&mut self, entry: &Entry) -> Result<bool> {
        if let Some(at) = self.retired.iter().position(|w| w.key == entry.key) {
            if self.retired[at].kernel.get() as usize != entry.overlapped {
                return Err(super::invalid());
            }
            // The last request of a released watch: storage may now be freed.
            drop(self.retired.swap_remove(at));
            return Ok(true);
        }
        let Some(&i) = self
            .active
            .iter()
            .find(|&&i| self.entries[i].as_ref().is_some_and(|w| w.key == entry.key))
        else {
            return Ok(false);
        };
        let watch = self.entries[i].as_mut().expect("active watch");
        if !watch.armed || watch.kernel.get() as usize != entry.overlapped {
            return Err(super::invalid());
        }
        watch.armed = false;
        let h = watch.handle;
        if watch.cancelling {
            // Cancellation acknowledged (or raced with a completion): report Cancelled.
            self.finish(h);
            return Ok(true);
        }
        if entry.status < 0 {
            // Includes an unrequested STATUS_CANCELLED: the watch cannot continue.
            // SAFETY: pure NTSTATUS conversion with no pointers.
            let code = unsafe { RtlNtStatusToDosError(entry.status) };
            let error: Error = std::io::Error::from_raw_os_error(code as i32).into();
            if let Some(op) = watch.op.take() {
                self.ops[op.index()] = None;
                self.finished.push_back((op, Err(error)));
            }
            return Ok(true);
        }
        watch.parse(entry.bytes as usize);
        if let Err(error) = watch.arm() {
            if let Some(op) = watch.op.take() {
                self.ops[op.index()] = None;
                self.finished.push_back((op, Err(error)));
            }
            return Ok(true);
        }
        if watch.op.is_some() && !watch.queued && !watch.ring.is_empty() {
            watch.queued = true;
            self.ready.push_back(h);
        }
        Ok(true)
    }
    pub fn has_work(&self) -> bool {
        !self.finished.is_empty() || (!self.ready.is_empty() && self.pool.available())
    }
    pub fn collect(&mut self, events: &mut Vec<Event<Detached>>) {
        while events.len() < events.capacity() {
            let Some((op, result)) = self.finished.pop_front() else {
                break;
            };
            events.push(Event {
                op,
                terminal: true,
                result: result.map(|()| Outcome::Cancelled),
            });
        }
        for _ in 0..self.ready.len() {
            if events.len() == events.capacity() {
                break;
            }
            let Some(h) = self.ready.pop_front() else {
                break;
            };
            let Some(watch) = self.get_mut(h) else {
                continue;
            };
            let Some(op) = watch.op.filter(|_| !watch.cancelling) else {
                watch.queued = false;
                continue;
            };
            if watch.ring.is_empty() {
                watch.queued = false;
                continue;
            }
            let Some(mut lease) = self.pool.acquire() else {
                self.ready.push_front(h);
                break;
            };
            let watch = self.get_mut(h).expect("ready watch");
            let overflow = watch.ring.take(&mut lease);
            if watch.ring.is_empty() {
                watch.queued = false;
            } else {
                self.ready.push_back(h);
            }
            events.push(Event {
                op,
                terminal: false,
                result: Ok(Outcome::Watch {
                    events: lease,
                    overflow,
                }),
            });
        }
    }
    /// Release after Closed (or a failed start/submit); pending I/O retires the storage.
    pub fn release(&mut self, h: Handle) {
        let Some(watch) = self.entries.get_mut(h.index()).and_then(|slot| {
            if slot.as_ref().is_some_and(|w| w.handle == h) {
                slot.take()
            } else {
                None
            }
        }) else {
            return;
        };
        if let Some(at) = self.active.iter().position(|&i| i == h.index()) {
            self.active.swap_remove(at);
        }
        if watch.queued {
            self.ready.retain(|&queued| queued != h);
        }
        if let Some(op) = watch.op {
            self.ops[op.index()] = None;
        }
        if watch.armed {
            if watch.cancel_native().is_err() {
                // Storage must outlive a request that could not be cancelled.
                std::process::abort();
            }
            self.retired.push(watch);
        }
    }
    /// Cancel every request before the backend drains its port on drop.
    pub fn shutdown(&mut self) {
        for &i in &self.active {
            let watch = self.entries[i].as_mut().expect("active watch");
            if watch.cancel_native().is_err() {
                std::process::abort();
            }
            watch.cancelling = true;
        }
    }
    /// Whether any kernel request still references watch storage.
    pub fn pending(&self) -> bool {
        !self.retired.is_empty()
            || self
                .active
                .iter()
                .any(|&i| self.entries[i].as_ref().is_some_and(|w| w.armed))
    }
}
