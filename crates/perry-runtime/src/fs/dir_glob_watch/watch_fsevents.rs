//! FSEvents backend for `fs.watch` on macOS, bound at RUNTIME through
//! `dlopen` (#9591).
//!
//! Why not `notify`'s FSEvents backend: `fsevent-sys` declares
//! `#[link(name = "CoreServices", kind = "framework")]`, and that metadata does
//! not survive perry's custom link step, so every binary whose link retains
//! the watcher — which is every binary that imports `fs`; the module table
//! pins `js_fs_watch` — would need `-framework CoreServices` on its link line.
//! An extra `LC_LOAD_DYLIB` costs launch time (`link/build_and_run.rs`: ~1.8 ms
//! for CoreFoundation + objc on a 3 ms hello world, and CoreServices is an
//! umbrella that drags CoreFoundation and more), which perry deliberately
//! keeps off runtime-only console binaries. Binding the ten symbols we need
//! at the first `fs.watch` call keeps that property: a program that never
//! watches never loads the framework.
//!
//! Semantics follow libuv's `uv__fsevents_event_cb`: one event per reported
//! path, `'rename'` when any of Created / Removed / Renamed / RootChanged is
//! set, otherwise `'change'` when any of the modification flags is set;
//! latency 0.05 s so a burst coalesces the way it does under node. Events
//! arrive on a private dispatch queue (libdispatch lives in libSystem: no
//! CFRunLoop thread, no further dylib) and each callback pushes onto the
//! per-thread [`EventQueue`] like every other cross-thread producer.
//!
//! One instance owns one stream over a set of paths. FSEvents streams are
//! immutable, so adding or removing a path rebuilds the stream (stop,
//! invalidate, release, create, start) — the same thing libuv and notify do.

use std::ffi::{c_char, c_void, CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use super::watch_backend::{EventClass, EventQueue, RawEvent, Source, WatchError};

type CFRef = *const c_void;
type StreamRef = *mut c_void;
type DispatchQueue = *mut c_void;

const CORE_FOUNDATION: &[u8] =
    b"/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation\0";
const CORE_SERVICES: &[u8] = b"/System/Library/Frameworks/CoreServices.framework/CoreServices\0";

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW: u64 = u64::MAX;
/// libuv's latency: events within this window coalesce per path.
const STREAM_LATENCY_SECONDS: f64 = 0.05;
const CREATE_FLAG_FILE_EVENTS: u32 = 0x0000_0010;

const EVENT_FLAG_ROOT_CHANGED: u32 = 0x0000_0020;
const EVENT_FLAG_ITEM_CREATED: u32 = 0x0000_0100;
const EVENT_FLAG_ITEM_REMOVED: u32 = 0x0000_0200;
const EVENT_FLAG_ITEM_INODE_META_MOD: u32 = 0x0000_0400;
const EVENT_FLAG_ITEM_RENAMED: u32 = 0x0000_0800;
const EVENT_FLAG_ITEM_MODIFIED: u32 = 0x0000_1000;
const EVENT_FLAG_ITEM_FINDER_INFO_MOD: u32 = 0x0000_2000;
const EVENT_FLAG_ITEM_CHANGE_OWNER: u32 = 0x0000_4000;
const EVENT_FLAG_ITEM_XATTR_MOD: u32 = 0x0000_8000;

const RENAME_FLAGS: u32 = EVENT_FLAG_ITEM_CREATED
    | EVENT_FLAG_ITEM_REMOVED
    | EVENT_FLAG_ITEM_RENAMED
    | EVENT_FLAG_ROOT_CHANGED;
const CHANGE_FLAGS: u32 = EVENT_FLAG_ITEM_MODIFIED
    | EVENT_FLAG_ITEM_INODE_META_MOD
    | EVENT_FLAG_ITEM_FINDER_INFO_MOD
    | EVENT_FLAG_ITEM_CHANGE_OWNER
    | EVENT_FLAG_ITEM_XATTR_MOD;

/// libuv's classification of one FSEvents flag word: rename wins, then
/// change, else the event carries no action (HistoryDone, MustScanSubDirs,
/// bare IsFile/IsDir markers) and is dropped.
pub(super) fn classify_flags(flags: u32) -> Option<EventClass> {
    if flags & RENAME_FLAGS != 0 {
        Some(EventClass::Rename)
    } else if flags & CHANGE_FLAGS != 0 {
        Some(EventClass::Change)
    } else {
        None
    }
}

#[repr(C)]
struct StreamContext {
    version: isize,
    info: *mut c_void,
    retain: Option<unsafe extern "C" fn(*const c_void) -> *const c_void>,
    release: Option<unsafe extern "C" fn(*const c_void)>,
    copy_description: Option<unsafe extern "C" fn(*const c_void) -> CFRef>,
}

type StreamCallback =
    unsafe extern "C" fn(StreamRef, *mut c_void, usize, *mut c_void, *const u32, *const u64);

/// The CoreFoundation / CoreServices entry points, resolved once.
struct Api {
    cf_string_create_with_cstring: unsafe extern "C" fn(CFRef, *const c_char, u32) -> CFRef,
    cf_array_create: unsafe extern "C" fn(CFRef, *const CFRef, isize, *const c_void) -> CFRef,
    cf_release: unsafe extern "C" fn(CFRef),
    cf_type_array_callbacks: *const c_void,
    stream_create: unsafe extern "C" fn(
        CFRef,
        StreamCallback,
        *const StreamContext,
        CFRef,
        u64,
        f64,
        u32,
    ) -> StreamRef,
    stream_set_dispatch_queue: unsafe extern "C" fn(StreamRef, DispatchQueue),
    stream_start: unsafe extern "C" fn(StreamRef) -> u8,
    stream_stop: unsafe extern "C" fn(StreamRef),
    stream_invalidate: unsafe extern "C" fn(StreamRef),
    stream_release: unsafe extern "C" fn(StreamRef),
}

// SAFETY: function pointers and one immutable data address into a framework
// that stays mapped for the life of the process.
unsafe impl Send for Api {}
unsafe impl Sync for Api {}

extern "C" {
    // libdispatch is part of libSystem — always linked, no new image.
    fn dispatch_queue_create(label: *const c_char, attr: *const c_void) -> DispatchQueue;
    fn dispatch_release(object: *mut c_void);
}

fn unavailable(what: &str) -> WatchError {
    WatchError {
        errno: None,
        message: format!("FSEvents unavailable: {what}"),
    }
}

unsafe fn open_image(path: &[u8]) -> Result<*mut c_void, WatchError> {
    let handle = libc::dlopen(
        path.as_ptr() as *const c_char,
        libc::RTLD_LAZY | libc::RTLD_LOCAL,
    );
    if handle.is_null() {
        let name = CStr::from_bytes_with_nul(path)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(unavailable(&format!("cannot load {name}")));
    }
    Ok(handle)
}

unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &[u8]) -> Result<T, WatchError> {
    let ptr = libc::dlsym(handle, name.as_ptr() as *const c_char);
    if ptr.is_null() {
        let name = CStr::from_bytes_with_nul(name)
            .map(|c| c.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Err(unavailable(&format!("missing symbol {name}")));
    }
    // SAFETY: T is a pointer-sized function pointer or `*const c_void`.
    Ok(std::mem::transmute_copy::<*mut c_void, T>(&ptr))
}

unsafe fn load_api() -> Result<Api, WatchError> {
    let cf = open_image(CORE_FOUNDATION)?;
    let cs = open_image(CORE_SERVICES)?;
    Ok(Api {
        cf_string_create_with_cstring: symbol(cf, b"CFStringCreateWithCString\0")?,
        cf_array_create: symbol(cf, b"CFArrayCreate\0")?,
        cf_release: symbol(cf, b"CFRelease\0")?,
        cf_type_array_callbacks: symbol(cf, b"kCFTypeArrayCallBacks\0")?,
        stream_create: symbol(cs, b"FSEventStreamCreate\0")?,
        stream_set_dispatch_queue: symbol(cs, b"FSEventStreamSetDispatchQueue\0")?,
        stream_start: symbol(cs, b"FSEventStreamStart\0")?,
        stream_stop: symbol(cs, b"FSEventStreamStop\0")?,
        stream_invalidate: symbol(cs, b"FSEventStreamInvalidate\0")?,
        stream_release: symbol(cs, b"FSEventStreamRelease\0")?,
    })
}

fn api() -> Result<&'static Api, WatchError> {
    static API: OnceLock<Result<Api, WatchError>> = OnceLock::new();
    API.get_or_init(|| unsafe { load_api() })
        .as_ref()
        .map_err(Clone::clone)
}

/// What the stream callback needs: where to push, and which instance it is.
struct Sink {
    source: Source,
    queue: Arc<EventQueue>,
}

unsafe extern "C" fn release_sink(info: *const c_void) {
    drop(Arc::from_raw(info as *const Sink));
}

unsafe extern "C" fn stream_callback(
    _stream: StreamRef,
    info: *mut c_void,
    num_events: usize,
    event_paths: *mut c_void,
    event_flags: *const u32,
    _event_ids: *const u64,
) {
    let sink = &*(info as *const Sink);
    let paths = event_paths as *const *const c_char;
    let mut batch = Vec::with_capacity(num_events);
    for i in 0..num_events {
        let Some(class) = classify_flags(*event_flags.add(i)) else {
            continue;
        };
        let bytes = CStr::from_ptr(*paths.add(i)).to_bytes();
        // Directory events may carry a trailing slash; the router compares
        // whole components against the canonical root.
        let trimmed = if bytes.len() > 1 && bytes.ends_with(b"/") {
            &bytes[..bytes.len() - 1]
        } else {
            bytes
        };
        batch.push(RawEvent::Native {
            source: sink.source,
            path: PathBuf::from(std::ffi::OsStr::from_bytes(trimmed)),
            class,
        });
    }
    sink.queue.push_all(batch);
}

/// One FSEvents stream over a set of watched paths.
pub(super) struct NativeInstance {
    api: &'static Api,
    sink: Arc<Sink>,
    dispatch_queue: DispatchQueue,
    paths: Vec<PathBuf>,
    stream: StreamRef,
}

// SAFETY: the raw handles are only ever used from the owning thread; FSEvents
// delivers on its own dispatch queue through `Sink`, which is `Send + Sync`.
unsafe impl Send for NativeInstance {}

impl NativeInstance {
    pub(super) fn new(source: Source, queue: Arc<EventQueue>) -> Result<Self, WatchError> {
        let api = api()?;
        let label = b"perry.fs.watch\0";
        let dispatch_queue =
            unsafe { dispatch_queue_create(label.as_ptr() as *const c_char, std::ptr::null()) };
        if dispatch_queue.is_null() {
            return Err(unavailable("dispatch_queue_create failed"));
        }
        Ok(NativeInstance {
            api,
            sink: Arc::new(Sink { source, queue }),
            dispatch_queue,
            paths: Vec::new(),
            stream: std::ptr::null_mut(),
        })
    }

    /// FSEvents streams are always recursive; the router applies the
    /// non-recursive depth rule, so `recursive` is not needed here.
    pub(super) fn watch(&mut self, root: &Path, _recursive: bool) -> Result<(), WatchError> {
        if !self.paths.iter().any(|p| p == root) {
            self.paths.push(root.to_path_buf());
        }
        self.rebuild()
    }

    pub(super) fn unwatch(&mut self, root: &Path) {
        self.paths.retain(|p| p != root);
        let _ = self.rebuild();
    }

    fn teardown(&mut self) {
        if self.stream.is_null() {
            return;
        }
        unsafe {
            (self.api.stream_stop)(self.stream);
            (self.api.stream_invalidate)(self.stream);
            (self.api.stream_release)(self.stream);
        }
        self.stream = std::ptr::null_mut();
    }

    fn rebuild(&mut self) -> Result<(), WatchError> {
        self.teardown();
        if self.paths.is_empty() {
            return Ok(());
        }
        let api = self.api;
        unsafe {
            let mut cf_paths: Vec<CFRef> = Vec::with_capacity(self.paths.len());
            for path in &self.paths {
                let c_path = CString::new(path.as_os_str().as_bytes())
                    .map_err(|_| unavailable("path contains a NUL byte"))?;
                let cf_path = (api.cf_string_create_with_cstring)(
                    std::ptr::null(),
                    c_path.as_ptr(),
                    K_CF_STRING_ENCODING_UTF8,
                );
                if cf_path.is_null() {
                    for created in cf_paths {
                        (api.cf_release)(created);
                    }
                    return Err(unavailable("CFStringCreateWithCString failed"));
                }
                cf_paths.push(cf_path);
            }
            let array = (api.cf_array_create)(
                std::ptr::null(),
                cf_paths.as_ptr(),
                cf_paths.len() as isize,
                api.cf_type_array_callbacks,
            );
            // The array retains its members.
            for cf_path in cf_paths {
                (api.cf_release)(cf_path);
            }
            if array.is_null() {
                return Err(unavailable("CFArrayCreate failed"));
            }
            // One strong reference per stream, handed to FSEvents; `release_sink`
            // returns it when the stream is released.
            let info = Arc::into_raw(Arc::clone(&self.sink)) as *mut c_void;
            let context = StreamContext {
                version: 0,
                info,
                retain: None,
                release: Some(release_sink),
                copy_description: None,
            };
            let stream = (api.stream_create)(
                std::ptr::null(),
                stream_callback,
                &context,
                array,
                K_FS_EVENT_STREAM_EVENT_ID_SINCE_NOW,
                STREAM_LATENCY_SECONDS,
                CREATE_FLAG_FILE_EVENTS,
            );
            (api.cf_release)(array);
            if stream.is_null() {
                drop(Arc::from_raw(info as *const Sink));
                return Err(unavailable("FSEventStreamCreate failed"));
            }
            (api.stream_set_dispatch_queue)(stream, self.dispatch_queue);
            if (api.stream_start)(stream) == 0 {
                (api.stream_invalidate)(stream);
                (api.stream_release)(stream);
                return Err(unavailable("FSEventStreamStart failed"));
            }
            self.stream = stream;
        }
        Ok(())
    }
}

impl Drop for NativeInstance {
    fn drop(&mut self) {
        self.teardown();
        unsafe {
            dispatch_release(self.dispatch_queue);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_classify_like_libuv() {
        assert_eq!(
            classify_flags(EVENT_FLAG_ITEM_CREATED | EVENT_FLAG_ITEM_MODIFIED),
            Some(EventClass::Rename),
            "a create coalesced with its first write is one 'rename', as under node"
        );
        assert_eq!(
            classify_flags(EVENT_FLAG_ITEM_MODIFIED),
            Some(EventClass::Change)
        );
        assert_eq!(
            classify_flags(EVENT_FLAG_ITEM_XATTR_MOD),
            Some(EventClass::Change)
        );
        assert_eq!(
            classify_flags(EVENT_FLAG_ITEM_REMOVED),
            Some(EventClass::Rename)
        );
        assert_eq!(
            classify_flags(EVENT_FLAG_ROOT_CHANGED),
            Some(EventClass::Rename)
        );
        assert_eq!(classify_flags(0x0001 /* MustScanSubDirs */), None);
        assert_eq!(classify_flags(0x0001_0000 /* ItemIsFile alone */), None);
    }

    #[test]
    fn the_framework_binds_at_runtime() {
        // No `-framework CoreServices` on any link line: the ten entry points
        // resolve through dlopen/dlsym on the running system.
        let api = api().expect("CoreServices + CoreFoundation resolve via dlopen");
        assert!(!api.cf_type_array_callbacks.is_null());
    }
}
