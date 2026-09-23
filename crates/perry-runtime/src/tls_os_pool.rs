//! Android storage for `perry_thread_local!` (#10219).
//!
//! Rust's Android TLS backend consumes one of bionic's 128 pthread keys for
//! every declaration it touches, including const, non-Drop declarations. The
//! runtime alone can exhaust that process-wide pool while starting the UI pump.
//! This backend owns one pthread key and lazily allocates typed values behind
//! it. The existing HotKey cache still supplies the steady-state fast path.
//!
//! Entries and values have stable System-allocated addresses. No collection
//! borrow survives an initializer or destructor. Destructors run in reverse
//! initialization order and can access other live keys or initialize new keys;
//! destroyed keys cannot be resurrected. Non-Drop values, including HotTls,
//! remain alive until all user destructors have run. The pthread destructor
//! retains a tombstone through later POSIX destructor passes.
use std::alloc::{GlobalAlloc, Layout, System};
use std::marker::PhantomData;
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessError;
impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("thread-local value is being or has been destroyed")
    }
}
impl std::error::Error for AccessError {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Vacant,
    Initializing,
    Ready,
    Destroyed,
}
struct Entry {
    state: State,
    value: *mut u8,
    destroy: unsafe fn(*mut u8),
    next: *mut Entry,
    drop_next: *mut Entry,
}
struct Pool {
    entries: *mut Entry,
    drops: *mut Entry,
    slots: *mut *mut Entry,
    capacity: usize,
}
static NEXT_ID: Mutex<usize> = Mutex::new(0);
static KEY: OnceLock<libc::pthread_key_t> = OnceLock::new();
const DESTROYED: *mut libc::c_void = ptr::without_provenance_mut(1);

// System allocation avoids recursing through a global allocator's own TLS.
unsafe fn alloc<T>(value: T) -> *mut T {
    let layout = Layout::new::<T>();
    let out = if layout.size() == 0 {
        ptr::NonNull::<T>::dangling().as_ptr()
    } else {
        let out = System.alloc(layout).cast::<T>();
        if out.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        out
    };
    out.write(value);
    out
}
unsafe fn destroy<T>(value: *mut u8) {
    let value = value.cast::<T>();
    ptr::drop_in_place(value);
    if std::mem::size_of::<T>() != 0 {
        System.dealloc(value.cast(), Layout::new::<T>());
    }
}
fn key() -> libc::pthread_key_t {
    *crate::once_init::get_or_init(&KEY, || {
        let mut key = 0;
        if unsafe { libc::pthread_key_create(&mut key, Some(drop_pool)) } != 0 {
            panic!("cannot allocate the shared thread-local key");
        }
        key
    })
}
/// HotKey must report teardown before consulting its cached HotTls pointer.
#[cfg(target_os = "android")]
pub(crate) fn is_destroyed() -> bool {
    KEY.get()
        .is_some_and(|key| unsafe { libc::pthread_getspecific(*key) } == DESTROYED)
}

fn pool() -> Result<*mut Pool, AccessError> {
    let key = key();
    let value = unsafe { libc::pthread_getspecific(key) };
    if value == DESTROYED {
        return Err(AccessError);
    }
    if !value.is_null() {
        return Ok(value.cast());
    }
    let value = unsafe {
        alloc(Pool {
            entries: ptr::null_mut(),
            drops: ptr::null_mut(),
            slots: ptr::null_mut(),
            capacity: 0,
        })
    };
    if unsafe { libc::pthread_setspecific(key, value.cast()) } != 0 {
        unsafe {
            destroy::<Pool>(value.cast());
        }
        panic!("cannot publish the shared thread-local storage");
    }
    Ok(value)
}
// The index table grows without moving any Entry or T. Its allocation also
// uses System so lookup remains safe inside a global allocator's TLS setup.
unsafe fn grow_index_table(pool: *mut Pool, index: usize) {
    if index < (*pool).capacity {
        return;
    }
    let capacity = index
        .checked_add(1)
        .and_then(usize::checked_next_power_of_two)
        .expect("thread-local index table overflow")
        .max(8);
    let layout = Layout::array::<*mut Entry>(capacity).expect("thread-local index table layout");
    let slots = System.alloc(layout).cast::<*mut Entry>();
    if slots.is_null() {
        std::alloc::handle_alloc_error(layout);
    }
    for offset in 0..capacity {
        // GC_STORE_AUDIT(POINTER_FREE): an empty native metadata slot has no heap edge.
        slots.add(offset).write(ptr::null_mut());
    }
    if (*pool).capacity != 0 {
        ptr::copy_nonoverlapping((*pool).slots, slots, (*pool).capacity);
        System.dealloc(
            (*pool).slots.cast(),
            Layout::array::<*mut Entry>((*pool).capacity).unwrap(),
        );
    }
    (*pool).slots = slots;
    (*pool).capacity = capacity;
}

unsafe extern "C" fn drop_pool(value: *mut libc::c_void) {
    let key = *KEY.get().unwrap();
    // Keep a tombstone through every POSIX destructor pass: another library's
    // destructor must not resurrect storage that we have already released.
    if value == DESTROYED {
        libc::pthread_setspecific(key, DESTROYED);
        return;
    }
    let result = std::panic::catch_unwind(|| {
        let pool = value.cast::<Pool>();
        if libc::pthread_setspecific(key, value) != 0 {
            std::process::abort();
        }
        while !(*pool).drops.is_null() {
            let entry = (*pool).drops;
            (*pool).drops = (*entry).drop_next;
            (*entry).state = State::Destroyed;
            ((*entry).destroy)((*entry).value);
        }
        // No caller may rediscover the pool once its final allocations are freed.
        if libc::pthread_setspecific(key, DESTROYED) != 0 {
            std::process::abort();
        }
        // Values without Drop stay available to the destructors above. This
        // includes the hot-pointer cache used to retire cached slot addresses.
        let mut entry = (*pool).entries;
        while !entry.is_null() {
            let next = (*entry).next;
            if (*entry).state == State::Ready {
                ((*entry).destroy)((*entry).value);
            }
            destroy::<Entry>(entry.cast());
            entry = next;
        }
        if (*pool).capacity != 0 {
            System.dealloc(
                (*pool).slots.cast(),
                Layout::array::<*mut Entry>((*pool).capacity).unwrap(),
            );
        }
        destroy::<Pool>(pool.cast());
    });
    if result.is_err() {
        std::process::abort();
    }
}

pub struct LocalKey<T: 'static> {
    index: AtomicUsize,
    initialize: fn() -> T,
    marker: PhantomData<T>,
}
// Handles contain no T; each access resolves the calling thread's storage.
unsafe impl<T> Sync for LocalKey<T> {}
impl<T: 'static> LocalKey<T> {
    pub const fn new(initialize: fn() -> T) -> Self {
        Self {
            index: AtomicUsize::new(usize::MAX),
            initialize,
            marker: PhantomData,
        }
    }
    fn index(&self) -> usize {
        let index = self.index.load(Ordering::Relaxed);
        if index != usize::MAX {
            return index;
        }
        // Claim once per declaration, not per racing first-touching thread.
        // Release the lock before any allocation, initializer, or destructor.
        let mut next = NEXT_ID.lock().unwrap_or_else(|error| error.into_inner());
        let index = self.index.load(Ordering::Relaxed);
        if index != usize::MAX {
            return index;
        }
        let index = *next;
        *next = next
            .checked_add(1)
            .expect("thread-local declaration index overflow");
        self.index.store(index, Ordering::Relaxed);
        index
    }
    pub fn with<F, R>(&'static self, f: F) -> R
    where
        F: FnOnce(&T) -> R,
    {
        self.try_with(f)
            .expect("cannot access destroyed thread-local storage")
    }
    pub fn try_with<F, R>(&'static self, f: F) -> Result<R, AccessError>
    where
        F: FnOnce(&T) -> R,
    {
        let pool = pool()?;
        let index = self.index();
        unsafe {
            grow_index_table(pool, index);
            let mut entry = *(*pool).slots.add(index);
            if entry.is_null() {
                entry = alloc(Entry {
                    state: State::Vacant,
                    value: ptr::null_mut(),
                    destroy: destroy::<T>,
                    next: (*pool).entries,
                    drop_next: ptr::null_mut(),
                });
                // GC_STORE_AUDIT(POINTER_FREE): links System-allocated TLS
                // metadata, not a Perry heap reference. The values retain
                // their existing type-specific root scanners.
                (*pool).entries = entry;
                (*pool).slots.add(index).write(entry);
            }
            match (*entry).state {
                State::Destroyed => return Err(AccessError),
                State::Initializing => panic!("recursive thread-local initialization"),
                State::Ready => {}
                State::Vacant => {
                    (*entry).state = State::Initializing;
                    struct Reset(*mut Entry);
                    impl Drop for Reset {
                        fn drop(&mut self) {
                            unsafe {
                                (*self.0).state = State::Vacant;
                            }
                        }
                    }
                    let reset = Reset(entry);
                    // Hold no reference or collection borrow across user code.
                    let value = alloc((self.initialize)());
                    (*entry).value = value.cast();
                    (*entry).state = State::Ready;
                    if std::mem::needs_drop::<T>() {
                        (*entry).drop_next = (*pool).drops;
                        (*pool).drops = entry;
                    }
                    std::mem::forget(reset);
                }
            }
            Ok(f(&*(*entry).value.cast::<T>()))
        }
    }
}

#[cfg(test)]
mod tests;
