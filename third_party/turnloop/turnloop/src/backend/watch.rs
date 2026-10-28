//! Filesystem watches for the epoll and kqueue backends, without turnloop threads.
//!
//! * Linux/Android: one inotify descriptor per loop, registered with epoll.
//!   Watches of the same inode share a watch descriptor.
//! * kqueue (BSD, iOS, and files on macOS): EVFILT_VNODE on the loop's kqueue.
//! * macOS directories: FSEvents streams on a private serial dispatch queue (the
//!   OS delivers on its own threads) that publish to this loop and notify it.
//!
//! Records are parsed as the OS reports them into bounded per-watch storage and
//! delivered as pooled batches. Recursion is available only through FSEvents.
use super::{
    poller::{Poller, Ready},
    unix::Detached,
};
use crate::slots::{Slots, page_reserve};
use crate::{
    backend::{Event, Operation, Outcome, Request},
    fs::watch::Ring,
    *,
};
use std::{collections::VecDeque, os::fd::AsRawFd};
#[cfg(target_os = "macos")]
#[path = "fsevents.rs"]
mod fsevents;

#[cfg(any(target_os = "linux", target_os = "android"))]
/// Reserved poller key of the loop's inotify descriptor; no handle key has an
/// index of 0xFFFF_FFFE.
const INOTIFY_KEY: u64 = u64::MAX - 1;

enum Source {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    Inotify { wd: i32 },
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    Vnode(std::os::fd::OwnedFd),
    #[cfg(target_os = "macos")]
    FsEvents(fsevents::Stream),
}
struct Entry {
    handle: Handle,
    op: Option<OpId>,
    queued: bool,
    /// Final path component, reported for events on the watched root itself.
    name: Box<[u8]>,
    /// Records of loop-thread sources (FSEvents keeps its own, shared with its queue).
    ring: Ring,
    source: Source,
}
impl Entry {
    fn with_ring<R>(&mut self, f: impl FnOnce(&mut Ring) -> R) -> R {
        #[cfg(target_os = "macos")]
        if let Source::FsEvents(stream) = &self.source {
            return stream.with_ring(f);
        }
        f(&mut self.ring)
    }
}
pub(super) struct Watches {
    entries: Slots<Entry>,
    /// Indices of occupied entries, so event dispatch never scans every handle slot.
    active: Vec<usize>,
    ops: Slots<Handle>,
    ready: VecDeque<Handle>,
    cancelled: VecDeque<OpId>,
    pool: BufferPool,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    inotify: Option<Inotify>,
    #[cfg(target_os = "macos")]
    fsevents: fsevents::Streams,
}
#[cfg(any(target_os = "linux", target_os = "android"))]
struct Inotify {
    fd: std::os::fd::OwnedFd,
    buffer: Box<[u8]>,
    readable: bool,
}

fn basename(path: &FsPath) -> Box<[u8]> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_path().as_os_str().as_bytes();
    let trimmed = match bytes.iter().rposition(|&b| b != b'/') {
        Some(end) => &bytes[..=end],
        None => return bytes.into(),
    };
    match trimmed.iter().rposition(|&b| b == b'/') {
        Some(slash) => trimmed[slash + 1..].into(),
        None => trimmed.into(),
    }
}

impl Watches {
    pub fn new(config: &Config, pool: BufferPool) -> Self {
        Self {
            entries: Slots::new(config.max_handles),
            active: Vec::with_capacity(page_reserve(config.max_handles)),
            ops: Slots::new(config.max_operations),
            ready: VecDeque::with_capacity(page_reserve(config.max_handles)),
            cancelled: VecDeque::with_capacity(page_reserve(config.max_operations)),
            pool,
            #[cfg(any(target_os = "linux", target_os = "android"))]
            inotify: None,
            #[cfg(target_os = "macos")]
            fsevents: fsevents::Streams::new(),
        }
    }
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables))]
    pub fn set_notifier(&mut self, notifier: Notifier) {
        #[cfg(target_os = "macos")]
        self.fsevents.set_notifier(notifier);
    }
    fn get(&self, h: Handle) -> Option<&Entry> {
        self.entries
            .get(h.index())
            .and_then(Option::as_ref)
            .filter(|e| e.handle == h)
    }
    pub fn contains(&self, h: Handle) -> bool {
        self.get(h).is_some()
    }
    pub fn start<P: Poller>(
        &mut self,
        h: Handle,
        path: &FsPath,
        options: WatchOptions,
        poller: &mut P,
    ) -> Result<()> {
        if self.entries.get(h.index()).is_none_or(Option::is_some) {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let source = self.source(h, path, options, poller)?;
        #[cfg(target_os = "macos")]
        let ring = if matches!(source, Source::FsEvents(_)) {
            Ring::empty()
        } else {
            Ring::new()
        };
        #[cfg(not(target_os = "macos"))]
        let ring = Ring::new();
        self.entries[h.index()] = Some(Entry {
            handle: h,
            op: None,
            queued: false,
            name: basename(path),
            ring,
            source,
        });
        self.active.push(h.index());
        Ok(())
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn source<P: Poller>(
        &mut self,
        _h: Handle,
        path: &FsPath,
        options: WatchOptions,
        poller: &mut P,
    ) -> Result<Source> {
        use std::os::fd::FromRawFd;
        if options.recursive {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        if self.inotify.is_none() {
            // SAFETY: integer flags; a successful call returns a new owned descriptor.
            let fd = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
            if fd < 0 {
                return Err(super::poller::last_error());
            }
            // SAFETY: inotify_init1 returned a new descriptor owned by nobody else.
            let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
            poller.register(fd.as_raw_fd(), INOTIFY_KEY)?;
            self.inotify = Some(Inotify {
                fd,
                buffer: vec![0; 64 * 1024].into_boxed_slice(),
                readable: false,
            });
        }
        let inotify = self.inotify.as_ref().expect("inotify");
        // As libuv: every event except attribute and content changes is a rename.
        let mask = libc::IN_ATTRIB
            | libc::IN_CREATE
            | libc::IN_MODIFY
            | libc::IN_DELETE
            | libc::IN_DELETE_SELF
            | libc::IN_MOVE_SELF
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO;
        // SAFETY: live inotify descriptor and NUL-terminated prepared path.
        let wd = unsafe {
            libc::inotify_add_watch(inotify.fd.as_raw_fd(), path.native().as_ptr(), mask)
        };
        if wd < 0 {
            return Err(super::poller::last_error());
        }
        Ok(Source::Inotify { wd })
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    fn source<P: Poller>(
        &mut self,
        h: Handle,
        path: &FsPath,
        options: WatchOptions,
        poller: &mut P,
    ) -> Result<Source> {
        #[cfg(target_os = "macos")]
        {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            // SAFETY: NUL-terminated path and writable output, read only after success.
            if unsafe { libc::stat(path.native().as_ptr(), stat.as_mut_ptr()) } < 0 {
                return Err(super::poller::last_error());
            }
            // SAFETY: successful stat initialized the structure.
            let directory =
                unsafe { stat.assume_init_ref() }.st_mode & libc::S_IFMT == libc::S_IFDIR;
            // As libuv: FSEvents for directories, kqueue for files.
            if directory {
                return self
                    .fsevents
                    .start(h, path, options.recursive)
                    .map(Source::FsEvents);
            }
        }
        if options.recursive {
            return Err(Error::new(ErrorKind::Unsupported));
        }
        vnode(h, path, poller).map(Source::Vnode)
    }
    pub fn submit(&mut self, request: &Request) -> Result<()> {
        let h = request.handle;
        if !matches!(request.operation, Operation::WatchFs)
            || self.get(h).is_none_or(|e| e.op.is_some())
            || self.ops.get(request.op.index()).is_none_or(Option::is_some)
        {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        let entry = self.entries[h.index()].as_mut().expect("watch entry");
        entry.op = Some(request.op);
        self.ops[request.op.index()] = Some(h);
        Ok(())
    }
    pub fn cancel(&mut self, op: OpId) -> bool {
        let Some(h) = self.ops.get(op.index()).copied().flatten() else {
            return false;
        };
        let Some(entry) = self.entries[h.index()]
            .as_mut()
            .filter(|e| e.handle == h && e.op == Some(op))
        else {
            return false;
        };
        entry.op = None;
        entry.with_ring(Ring::clear);
        self.ops[op.index()] = None;
        self.cancelled.push_back(op);
        true
    }
    /// Claim a poller readiness event belonging to a watch source.
    pub fn ready(&mut self, e: Ready) -> bool {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if e.key == INOTIFY_KEY {
            if let Some(inotify) = &mut self.inotify {
                inotify.readable = true;
            }
            return true;
        }
        let Some(entry) = self
            .entries
            .get_mut(e.key as u32 as usize)
            .and_then(Option::as_mut)
            .filter(|entry| entry.handle.key() == e.key)
        else {
            return false;
        };
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        if matches!(entry.source, Source::Vnode(_)) && entry.op.is_some() {
            let flags = e.vnode;
            if flags & (libc::NOTE_WRITE | libc::NOTE_EXTEND | libc::NOTE_ATTRIB) != 0 {
                entry.ring.push(WatchKind::Change, &entry.name);
            }
            if flags & (libc::NOTE_RENAME | libc::NOTE_DELETE | libc::NOTE_REVOKE) != 0 {
                entry.ring.push(WatchKind::Rename, &entry.name);
            }
            if !entry.queued && !entry.ring.is_empty() {
                entry.queued = true;
                self.ready.push_back(entry.handle);
            }
        }
        let _ = (e, entry);
        true
    }
    pub fn has_work(&self) -> bool {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        if self.inotify.as_ref().is_some_and(|i| i.readable) {
            return true;
        }
        #[cfg(target_os = "macos")]
        if self.fsevents.has_work() {
            return true;
        }
        !self.cancelled.is_empty() || (!self.ready.is_empty() && self.pool.available())
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn read_inotify(&mut self) {
        let Some(inotify) = &mut self.inotify else {
            return;
        };
        // Bound one poll's work; remaining readiness stays cached for the next.
        for _ in 0..16 {
            if !inotify.readable {
                break;
            }
            // SAFETY: live nonblocking descriptor and exclusively owned buffer.
            let n = unsafe {
                libc::read(
                    inotify.fd.as_raw_fd(),
                    inotify.buffer.as_mut_ptr().cast(),
                    inotify.buffer.len(),
                )
            };
            if n < 0 {
                let e = super::poller::last_error();
                if e.os != Some(libc::EINTR) {
                    // EAGAIN ends the edge; any other error is reported as loss.
                    inotify.readable = false;
                    if e.os != Some(libc::EAGAIN) {
                        for &i in &self.active {
                            self.entries[i].as_mut().expect("active watch").ring.lost();
                        }
                    }
                }
                continue;
            }
            let bytes = &inotify.buffer[..n as usize];
            let mut at = 0;
            let header = std::mem::size_of::<libc::inotify_event>();
            while at + header <= bytes.len() {
                // SAFETY: the kernel wrote a complete event header at this offset.
                let event: libc::inotify_event =
                    unsafe { std::ptr::read_unaligned(bytes[at..].as_ptr().cast()) };
                let name_end = (at + header + event.len as usize).min(bytes.len());
                let raw = &bytes[at + header..name_end];
                let name = &raw[..raw.iter().position(|&b| b == 0).unwrap_or(raw.len())];
                at = name_end;
                for &i in &self.active {
                    let entry = self.entries[i].as_mut().expect("active watch");
                    let Source::Inotify { wd } = &mut entry.source;
                    if event.mask & libc::IN_Q_OVERFLOW != 0 {
                        entry.ring.lost();
                    } else if *wd != event.wd {
                        continue;
                    } else if event.mask & libc::IN_IGNORED != 0 {
                        // The kernel removed this watch; a reused number must not match.
                        *wd = -1;
                        continue;
                    } else if entry.op.is_some() {
                        let kind = if event.mask & !(libc::IN_ATTRIB | libc::IN_MODIFY) != 0 {
                            WatchKind::Rename
                        } else {
                            WatchKind::Change
                        };
                        let name = if name.is_empty() { &entry.name } else { name };
                        entry.ring.push(kind, name);
                    }
                    if entry.op.is_some() && !entry.queued && !entry.ring.is_empty() {
                        entry.queued = true;
                        self.ready.push_back(entry.handle);
                    }
                }
            }
        }
    }
    pub fn poll(&mut self, events: &mut Vec<Event<Detached>>) {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        self.read_inotify();
        #[cfg(target_os = "macos")]
        self.fsevents.drain(|h| {
            let Some(entry) = self
                .entries
                .get_mut(h.index())
                .and_then(Option::as_mut)
                .filter(|e| e.handle == h)
            else {
                return;
            };
            if let Source::FsEvents(stream) = &entry.source {
                stream.rearm();
            }
            if entry.op.is_some() && !entry.queued {
                entry.queued = true;
                self.ready.push_back(h);
            }
        });
        while events.len() < events.capacity() {
            let Some(op) = self.cancelled.pop_front() else {
                break;
            };
            events.push(Event {
                op,
                terminal: true,
                result: Ok(Outcome::Cancelled),
            });
        }
        for _ in 0..self.ready.len() {
            if events.len() == events.capacity() {
                break;
            }
            let Some(h) = self.ready.pop_front() else {
                break;
            };
            let Some(entry) = self.entries[h.index()].as_mut().filter(|e| e.handle == h) else {
                continue;
            };
            let Some(op) = entry.op else {
                entry.queued = false;
                continue;
            };
            if entry.with_ring(|ring| ring.is_empty()) {
                entry.queued = false;
                continue;
            }
            let Some(mut lease) = self.pool.acquire() else {
                // Pool exhaustion: keep the records; resume when a lease returns.
                self.ready.push_front(h);
                break;
            };
            let (overflow, empty) =
                entry.with_ring(|ring| (ring.take(&mut lease), ring.is_empty()));
            if empty {
                entry.queued = false;
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
    pub fn release<P: Poller>(&mut self, h: Handle, poller: &mut P) {
        let Some(entry) = self.entries.get_mut(h.index()).and_then(|slot| {
            if slot.as_ref().is_some_and(|e| e.handle == h) {
                slot.take()
            } else {
                None
            }
        }) else {
            return;
        };
        let _ = &poller;
        if let Some(at) = self.active.iter().position(|&i| i == h.index()) {
            self.active.swap_remove(at);
        }
        if entry.queued {
            self.ready.retain(|&queued| queued != h);
        }
        match entry.source {
            #[cfg(any(target_os = "linux", target_os = "android"))]
            Source::Inotify { wd } => {
                let shared = self.active.iter().any(|&i| {
                    matches!(
                        self.entries[i].as_ref().expect("active watch").source,
                        Source::Inotify { wd: other } if other == wd
                    )
                });
                if wd >= 0
                    && !shared
                    && let Some(inotify) = &self.inotify
                {
                    // Bionic declares the watch descriptor `u32`, glibc/musl `c_int`;
                    // `wd >= 0` was checked above, so the conversion is lossless.
                    // SAFETY: live inotify descriptor and a watch owned by this loop.
                    unsafe { libc::inotify_rm_watch(inotify.fd.as_raw_fd(), wd as _) };
                }
            }
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            Source::Vnode(fd) => {
                // Closing the descriptor removes its knote from the kqueue.
                drop(fd);
            }
            #[cfg(target_os = "macos")]
            Source::FsEvents(stream) => self.fsevents.stop(h, stream),
        }
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn vnode<P: Poller>(h: Handle, path: &FsPath, poller: &mut P) -> Result<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;
    #[cfg(target_vendor = "apple")]
    let flags = libc::O_EVTONLY | libc::O_CLOEXEC;
    #[cfg(not(target_vendor = "apple"))]
    let flags = libc::O_RDONLY | libc::O_CLOEXEC;
    // SAFETY: NUL-terminated prepared path and integer flags.
    let fd = unsafe { libc::open(path.native().as_ptr(), flags) };
    if fd < 0 {
        return Err(super::poller::last_error());
    }
    // SAFETY: open returned a new descriptor owned by nobody else.
    let fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) };
    // SAFETY: kevent is plain C data; every field this filter reads is set below.
    let mut change: libc::kevent = unsafe { std::mem::zeroed() };
    change.ident = fd.as_raw_fd() as _;
    change.filter = libc::EVFILT_VNODE as _;
    change.flags = (libc::EV_ADD | libc::EV_CLEAR) as _;
    change.fflags = libc::NOTE_ATTRIB
        | libc::NOTE_WRITE
        | libc::NOTE_RENAME
        | libc::NOTE_DELETE
        | libc::NOTE_EXTEND
        | libc::NOTE_REVOKE;
    change.udata = h.key() as usize as _;
    // SAFETY: live kqueue and descriptor, one initialized change, no output wait.
    if unsafe {
        libc::kevent(
            poller.fd(),
            &change,
            1,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    } < 0
    {
        return Err(super::poller::last_error());
    }
    Ok(fd)
}
