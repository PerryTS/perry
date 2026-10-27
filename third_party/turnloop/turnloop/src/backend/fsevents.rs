//! FSEvents streams for macOS directory watches.
//!
//! CoreServices is loaded on first use (no launch-time framework dependency).
//! Streams deliver on one private serial dispatch queue per loop. A callback
//! parses events into the stream's bounded storage and publishes the handle to
//! this loop's inbox, notifying it; stopping a stream waits for the queue, so no
//! callback runs after `stop` returns.
use crate::{Error, ErrorKind, FsPath, Handle, Notifier, Result, WatchKind, fs::watch::Ring};
use std::{
    collections::VecDeque,
    ffi::{CStr, CString, c_char, c_void},
    os::unix::ffi::OsStrExt,
    sync::{
        Arc, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
};

type Ref = *const c_void;
type Callback = unsafe extern "C" fn(Ref, *mut c_void, usize, *mut c_void, *const u32, *const u64);
type StringFn = unsafe extern "C" fn(Ref, *const c_char) -> Ref;
type ArrayFn = unsafe extern "C" fn(Ref, *const Ref, isize, Ref) -> Ref;
type ReleaseFn = unsafe extern "C" fn(Ref);
type CreateFn =
    unsafe extern "C" fn(Ref, Callback, *const StreamContext, Ref, u64, f64, u32) -> Ref;
type QueueFn = unsafe extern "C" fn(Ref, Ref);
type StartFn = unsafe extern "C" fn(Ref) -> u8;
#[repr(C)]
struct StreamContext {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(Ref) -> Ref>,
    release: Option<unsafe extern "C" fn(Ref)>,
    description: Option<unsafe extern "C" fn(Ref) -> Ref>,
}
struct Api {
    string: StringFn,
    array: ArrayFn,
    cf_release: ReleaseFn,
    create: CreateFn,
    set_queue: QueueFn,
    start: StartFn,
    stop: ReleaseFn,
    invalidate: ReleaseFn,
    release: ReleaseFn,
}
// SAFETY: the table holds immutable function pointers of thread-safe framework APIs.
unsafe impl Send for Api {}
// SAFETY: see Send; the pointers are never mutated after loading.
unsafe impl Sync for Api {}
unsafe extern "C" {
    fn dispatch_queue_create(label: *const c_char, attr: Ref) -> Ref;
    fn dispatch_sync_f(queue: Ref, context: *mut c_void, work: unsafe extern "C" fn(*mut c_void));
    fn dispatch_release(object: Ref);
    fn dispatch_retain(object: Ref);
}
const SINCE_NOW: u64 = u64::MAX;
const NO_DEFER: u32 = 0x02;
const WATCH_ROOT: u32 = 0x04;
const FILE_EVENTS: u32 = 0x10;
const MUST_SCAN_SUBDIRS: u32 = 0x01;
const USER_DROPPED: u32 = 0x02;
const KERNEL_DROPPED: u32 = 0x04;
const ROOT_CHANGED: u32 = 0x20;
const RENAMED: u32 = 0x100 | 0x200 | 0x800; // created, removed, renamed
const MODIFIED: u32 = 0x400 | 0x1000 | 0x2000 | 0x4000 | 0x8000;
const IS_DIRECTORY: u32 = 0x20000;

fn api() -> Result<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| {
        let open = |path: &CStr| {
            // SAFETY: NUL-terminated system framework path; the image stays loaded.
            let image = unsafe { libc::dlopen(path.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
            (!image.is_null()).then_some(image)
        };
        let core = open(c"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")?;
        let services = open(c"/System/Library/Frameworks/CoreServices.framework/CoreServices")?;
        macro_rules! symbol {
            ($image:expr, $name:literal, $type:ty) => {{
                // SAFETY: NUL-terminated symbol name in a loaded image.
                let pointer = unsafe { libc::dlsym($image, $name.as_ptr()) };
                if pointer.is_null() {
                    return None;
                }
                // SAFETY: the named framework function has exactly the declared C ABI.
                unsafe { std::mem::transmute::<*mut c_void, $type>(pointer) }
            }};
        }
        Some(Api {
            string: symbol!(
                core,
                c"CFStringCreateWithFileSystemRepresentation",
                StringFn
            ),
            array: symbol!(core, c"CFArrayCreate", ArrayFn),
            cf_release: symbol!(core, c"CFRelease", ReleaseFn),
            create: symbol!(services, c"FSEventStreamCreate", CreateFn),
            set_queue: symbol!(services, c"FSEventStreamSetDispatchQueue", QueueFn),
            start: symbol!(services, c"FSEventStreamStart", StartFn),
            stop: symbol!(services, c"FSEventStreamStop", ReleaseFn),
            invalidate: symbol!(services, c"FSEventStreamInvalidate", ReleaseFn),
            release: symbol!(services, c"FSEventStreamRelease", ReleaseFn),
        })
    })
    .as_ref()
    .ok_or(Error::new(ErrorKind::Unsupported))
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Handles with published records, shared by every stream of one loop.
struct Inbox {
    pending: AtomicBool,
    ready: Mutex<VecDeque<Handle>>,
    notifier: Mutex<Option<Notifier>>,
}
struct Context {
    inbox: Arc<Inbox>,
    ring: Mutex<Ring>,
    queued: AtomicBool,
    handle: Handle,
    root: CString,
    name: Box<[u8]>,
    recursive: bool,
}

unsafe extern "C" fn receive(
    _stream: Ref,
    info: *mut c_void,
    count: usize,
    paths: *mut c_void,
    flags: *const u32,
    _ids: *const u64,
) {
    // SAFETY: the context outlives the stream (freed only after invalidation and
    // a queue barrier); FSEvents passes `count` C-string paths and flags.
    let (context, paths, flags) = unsafe {
        (
            &*info.cast::<Context>(),
            std::slice::from_raw_parts(paths.cast::<*const c_char>(), count),
            std::slice::from_raw_parts(flags, count),
        )
    };
    let root = context.root.as_bytes();
    let mut ring = lock(&context.ring);
    let mut published = false;
    for (&path, &flags) in paths.iter().zip(flags) {
        if flags & (MUST_SCAN_SUBDIRS | USER_DROPPED | KERNEL_DROPPED) != 0 {
            ring.lost();
            published = true;
        }
        if flags & ROOT_CHANGED != 0 {
            ring.push(WatchKind::Rename, &context.name);
            published = true;
            continue;
        }
        // SAFETY: FSEvents paths are NUL-terminated for the callback's duration.
        let path = unsafe { CStr::from_ptr(path) }.to_bytes();
        let Some(rest) = path.strip_prefix(root) else {
            continue;
        };
        let mut flags = flags;
        // Event naming follows libuv's FSEvents backend.
        let name = if rest.is_empty() {
            flags &= !RENAMED;
            &context.name[..]
        } else if root == b"/" {
            rest
        } else if let Some(child) = rest.strip_prefix(b"/") {
            child
        } else {
            continue;
        };
        if !context.recursive && !rest.is_empty() && name.contains(&b'/') {
            continue;
        }
        let kind = if flags & RENAMED == 0 && (flags & MODIFIED != 0 || flags & IS_DIRECTORY == 0) {
            WatchKind::Change
        } else {
            WatchKind::Rename
        };
        ring.push(kind, name);
        published = true;
    }
    drop(ring);
    if published && !context.queued.swap(true, Ordering::AcqRel) {
        let mut ready = lock(&context.inbox.ready);
        ready.push_back(context.handle);
        context.inbox.pending.store(true, Ordering::Release);
        drop(ready);
        if let Some(notifier) = lock(&context.inbox.notifier).as_ref() {
            let _ = notifier.notify();
        }
    }
}
unsafe extern "C" fn barrier(_: *mut c_void) {}

pub(super) struct Stream {
    stream: Ref,
    paths: Ref,
    string: Ref,
    /// Retained callback queue, used by Drop to wait for in-flight callbacks.
    queue: Ref,
    started: bool,
    context: Box<Context>,
}
impl Stream {
    /// Accept the next publication before this loop inspects the records.
    pub fn rearm(&self) {
        self.context.queued.store(false, Ordering::Release);
    }
    /// Access the records shared with the callback queue.
    pub fn with_ring<R>(&self, f: impl FnOnce(&mut Ring) -> R) -> R {
        f(&mut lock(&self.context.ring))
    }
}
// SAFETY: every exit path runs this Drop, which stops delivery before freeing the
// context: a loop dropped with a live watch cannot leave a callback behind.
impl Drop for Stream {
    fn drop(&mut self) {
        let Ok(api) = api() else {
            return;
        };
        // SAFETY: each object is owned by this stream and released exactly once.
        // A started stream is stopped and invalidated, then the loop thread (never
        // running on the private queue, so no deadlock) waits for the queue: no
        // callback can observe the context after this block.
        unsafe {
            if self.started {
                (api.stop)(self.stream);
                (api.invalidate)(self.stream);
            }
            if !self.queue.is_null() {
                dispatch_sync_f(self.queue, std::ptr::null_mut(), barrier);
                dispatch_release(self.queue);
            }
            if !self.stream.is_null() {
                (api.release)(self.stream);
            }
            if !self.paths.is_null() {
                (api.cf_release)(self.paths);
            }
            if !self.string.is_null() {
                (api.cf_release)(self.string);
            }
        }
    }
}

pub(super) struct Streams {
    inbox: Arc<Inbox>,
    queue: Ref,
}
impl Streams {
    pub fn new() -> Self {
        let inbox = Arc::new(Inbox {
            pending: AtomicBool::new(false),
            ready: Mutex::new(VecDeque::with_capacity(crate::slots::PAGE)),
            notifier: Mutex::new(None),
        });
        drop(lock(&inbox.ready));
        drop(lock(&inbox.notifier));
        Self {
            inbox,
            queue: std::ptr::null(),
        }
    }
    pub fn set_notifier(&mut self, notifier: Notifier) {
        *lock(&self.inbox.notifier) = Some(notifier);
    }
    pub fn has_work(&self) -> bool {
        self.inbox.pending.load(Ordering::Acquire)
    }
    pub fn drain(&mut self, mut ready: impl FnMut(Handle)) {
        if !self.has_work() {
            return;
        }
        let mut inbox = lock(&self.inbox.ready);
        self.inbox.pending.store(false, Ordering::Release);
        while let Some(h) = inbox.pop_front() {
            ready(h);
        }
    }
    pub fn start(&mut self, h: Handle, path: &FsPath, recursive: bool) -> Result<Stream> {
        let api = api()?;
        let root = std::fs::canonicalize(path.as_path()).map_err(Error::from)?;
        let root = CString::new(root.as_os_str().as_bytes())
            .map_err(|_| Error::new(ErrorKind::InvalidInput))?;
        let name = root
            .as_bytes()
            .rsplit(|&b| b == b'/')
            .find(|part| !part.is_empty())
            .unwrap_or(b"/")
            .into();
        if self.queue.is_null() {
            // SAFETY: static label; a null attribute creates a serial queue.
            self.queue =
                unsafe { dispatch_queue_create(c"turnloop.fsevents".as_ptr(), std::ptr::null()) };
            if self.queue.is_null() {
                return Err(Error::new(ErrorKind::ResourceLimit));
            }
        }
        let context = Box::new(Context {
            inbox: self.inbox.clone(),
            ring: Mutex::new(Ring::new()),
            queued: AtomicBool::new(false),
            handle: h,
            root,
            name,
            recursive,
        });
        drop(lock(&context.ring));
        // SAFETY: live queue; the stream holds its own reference until Drop.
        unsafe { dispatch_retain(self.queue) };
        let mut stream = Stream {
            stream: std::ptr::null(),
            paths: std::ptr::null(),
            string: std::ptr::null(),
            queue: self.queue,
            started: false,
            context,
        };
        // SAFETY: NUL-terminated canonical path; released by Stream::drop.
        stream.string = unsafe { (api.string)(std::ptr::null(), stream.context.root.as_ptr()) };
        if stream.string.is_null() {
            return Err(Error::new(ErrorKind::InvalidInput));
        }
        // SAFETY: one live CFString; the stream copies the array at creation.
        stream.paths =
            unsafe { (api.array)(std::ptr::null(), &stream.string, 1, std::ptr::null()) };
        if stream.paths.is_null() {
            return Err(Error::new(ErrorKind::ResourceLimit));
        }
        let info = StreamContext {
            version: 0,
            info: (&*stream.context as *const Context).cast_mut().cast(),
            retain: None,
            release: None,
            description: None,
        };
        // SAFETY: valid callback, context and path array. Zero latency with NoDefer
        // delivers without a driver-side interval; WatchRoot reports root moves.
        stream.stream = unsafe {
            (api.create)(
                std::ptr::null(),
                receive,
                &info,
                stream.paths,
                SINCE_NOW,
                0.0,
                NO_DEFER | WATCH_ROOT | FILE_EVENTS,
            )
        };
        if stream.stream.is_null() {
            return Err(Error::new(ErrorKind::Other));
        }
        // SAFETY: new, unscheduled stream and a live serial queue.
        unsafe { (api.set_queue)(stream.stream, self.queue) };
        stream.started = true;
        // SAFETY: scheduled stream whose boxed context stays pinned until Drop.
        if unsafe { (api.start)(stream.stream) } == 0 {
            return Err(Error::new(ErrorKind::Other));
        }
        Ok(stream)
    }
    /// Stop delivery, wait for in-flight callbacks and free the stream.
    pub fn stop(&mut self, h: Handle, stream: Stream) {
        drop(stream);
        lock(&self.inbox.ready).retain(|&queued| queued != h);
    }
}
impl Drop for Streams {
    fn drop(&mut self) {
        if !self.queue.is_null() {
            // SAFETY: this reference was created by dispatch_queue_create; streams hold their own.
            unsafe { dispatch_release(self.queue) };
        }
    }
}
