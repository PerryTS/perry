//! Owned, 8-aligned native bytes. The GC wrapper stays in its own heap;
//! only this allocation crosses threads. Allocation and free both use Rust's
//! process-global allocator (as SharedArrayBuffer does), including remote frees.
use std::alloc::{alloc_zeroed, dealloc, handle_alloc_error, Layout};
use std::sync::Mutex;

#[derive(Debug)]
pub(crate) struct Backing {
    data: *mut u8,
    capacity: u32,
}

// Exclusive ownership crosses the queue; no JS access remains after detach.
unsafe impl Send for Backing {}

impl Backing {
    pub(crate) fn zeroed(capacity: u32) -> Self {
        let layout = Self::layout(capacity);
        let data = unsafe { alloc_zeroed(layout) };
        if data.is_null() {
            handle_alloc_error(layout);
        }
        #[cfg(test)]
        LIVE_BACKINGS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self { data, capacity }
    }

    fn layout(capacity: u32) -> Layout {
        Layout::from_size_align((capacity as usize).max(1), 8).expect("buffer backing layout")
    }

    pub(crate) fn data(&self) -> *mut u8 {
        self.data
    }
    pub(crate) fn capacity(&self) -> u32 {
        self.capacity
    }

    pub(crate) unsafe fn copy(data: *const u8, length: u32) -> Self {
        let backing = Self::zeroed(length);
        if length != 0 {
            std::ptr::copy_nonoverlapping(data, backing.data, length as usize);
        }
        backing
    }
}

impl Drop for Backing {
    fn drop(&mut self) {
        unsafe { dealloc(self.data, Self::layout(self.capacity)) };
        #[cfg(test)]
        LIVE_BACKINGS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
pub(crate) static LIVE_BACKINGS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// A single-use transfer in the serialized tree. During the private writer
/// walk it names a source; commit removes that address before publication.
/// The queue owns the allocation until the reader takes it. Dropping an unread
/// message frees it. Cloning a message explicitly copies, rather than sharing
/// mutable bytes between independent receivers.
#[derive(Debug)]
pub struct TransferredBacking {
    source: usize,
    pub(crate) length: u32,
    backing: Mutex<Option<Backing>>,
}

impl TransferredBacking {
    pub(crate) fn pending(source: usize, length: u32) -> Self {
        Self {
            source,
            length,
            backing: Mutex::new(None),
        }
    }

    pub(crate) unsafe fn commit(&mut self) {
        let source = std::mem::replace(&mut self.source, 0);
        assert_ne!(source, 0);
        let backing = super::header::take_owned_backing(source).unwrap_or_else(|| {
            // Inline and addon-owned stores cannot leave their original owner.
            // Move to native storage once, after successful clone validation.
            super::bytes::no_gc(|scope| {
                let bytes =
                    super::bytes::bytes(crate::value::js_nanbox_pointer(source as i64), scope)
                        .expect("validated transfer source");
                Backing::copy(bytes.as_ptr(), self.length)
            })
        });
        #[cfg(test)]
        let backing = if super::bytes::b4_sabotage("transfer_copy") {
            Backing::copy(backing.data(), self.length)
        } else {
            backing
        };
        *self.backing.get_mut().unwrap() = Some(backing);
    }

    pub(crate) fn take(&self) -> Backing {
        assert_eq!(self.source, 0, "uncommitted transfer cannot cross threads");
        self.backing
            .lock()
            .unwrap()
            .take()
            .expect("transfer already received")
    }
}

impl Clone for TransferredBacking {
    fn clone(&self) -> Self {
        assert_eq!(self.source, 0, "private writer state cannot be cloned");
        let guard = self.backing.lock().unwrap();
        let original = guard.as_ref().expect("transfer already received");
        let backing = unsafe { Backing::copy(original.data(), self.length) };
        Self {
            source: 0,
            length: self.length,
            backing: Mutex::new(Some(backing)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{js_array_buffer_new, BufferHeader};
    use crate::gc::RuntimeHandleScope;
    use crate::thread::{deserialize_nanbox_on_current_thread, serialize_message};
    use crate::value::{JSValue, POINTER_MASK};
    use std::sync::atomic::Ordering;

    fn setup() -> std::sync::MutexGuard<'static, ()> {
        let lock = crate::gc::global_side_table_test_lock();
        crate::gc::register_runtime_handle_root_scanner_for_tests();
        lock
    }

    fn byte_address(buffer: *const BufferHeader) -> usize {
        crate::buffer::bytes::no_gc(|_| {
            crate::buffer::bytes::span(crate::value::js_nanbox_pointer(buffer as i64), false)
                .unwrap()
                .ptr as usize
        })
    }

    fn count() -> usize {
        LIVE_BACKINGS.load(Ordering::SeqCst)
    }
    fn collect() {
        crate::gc::js_gc_collect();
    }

    #[test]
    fn transfer_receiver_uses_original_pointer_after_sender_gc() {
        let _guard = setup();
        let before = count();
        let scope = RuntimeHandleScope::new();
        let source = js_array_buffer_new(32 * 1024 * 1024);
        let source_root = scope.root_raw_mut_ptr(source);
        let original = byte_address(source);
        unsafe {
            *(original as *mut u8) = 37;
            *(original as *mut u8).add(32 * 1024 * 1024 - 1) = 91;
            let message = serialize_message(
                JSValue::pointer(source.cast()).bits(),
                &[source as usize],
                None,
            )
            .unwrap();
            assert!(crate::buffer::is_detached_buffer(source as usize));
            assert_eq!((*source).length, 0);
            collect();
            assert_eq!(
                count(),
                before + 1,
                "message must own just the original store"
            );
            std::thread::spawn(move || {
                crate::gc::register_runtime_handle_root_scanner_for_tests();
                let scope = RuntimeHandleScope::new();
                let bits = deserialize_nanbox_on_current_thread(&message);
                let root = scope.root_nanbox_u64(bits);
                let received = (bits & POINTER_MASK) as *const BufferHeader;
                assert_eq!(
                    byte_address(received),
                    original,
                    "transfer must move the original allocation"
                );
                assert_eq!((*received).length, 32 * 1024 * 1024);
                drop(message);
                collect();
                let received = (root.get_nanbox_u64() & POINTER_MASK) as *const BufferHeader;
                assert_eq!(crate::buffer::js_buffer_get(received, 0), 37);
                crate::buffer::bytes::no_gc(|scope| {
                    let data = crate::buffer::bytes::bytes(
                        crate::value::js_nanbox_pointer(received as i64),
                        scope,
                    )
                    .unwrap();
                    assert_eq!(data.last(), Some(&91));
                });
            })
            .join()
            .unwrap();
        }
        assert_eq!(source_root.get_raw_mut_ptr::<BufferHeader>(), source);
        assert_eq!(count(), before, "worker exit must release received backing");
    }

    #[test]
    fn backing_outlives_its_allocating_worker() {
        let _guard = setup();
        let before = count();
        let (message, original) = std::thread::spawn(|| {
            let source = js_array_buffer_new(1024 * 1024);
            let original = byte_address(source);
            unsafe {
                *(original as *mut u8) = 81;
            }
            let message = unsafe {
                serialize_message(
                    JSValue::pointer(source.cast()).bits(),
                    &[source as usize],
                    None,
                )
                .unwrap()
            };
            (message, original)
        })
        .join()
        .unwrap();
        let scope = RuntimeHandleScope::new();
        let root = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
        let received = (root.get_nanbox_u64() & POINTER_MASK) as *const BufferHeader;
        assert_eq!(byte_address(received), original);
        assert_eq!(crate::buffer::js_buffer_get(received, 0), 81);
        crate::buffer::detach_array_buffer(received as usize);
        assert_eq!(count(), before);
    }

    #[test]
    fn unread_transfer_and_unused_transfer_list_release_backing() {
        let _guard = setup();
        let before = count();
        let source = js_array_buffer_new(1024 * 1024);
        let message = unsafe {
            serialize_message(
                JSValue::pointer(source.cast()).bits(),
                &[source as usize],
                None,
            )
            .unwrap()
        };
        assert_eq!(count(), before + 1);
        std::thread::spawn(move || drop(message)).join().unwrap();
        assert_eq!(
            count(),
            before,
            "discarded queue item must release on any thread"
        );
        let unused = js_array_buffer_new(1024 * 1024);
        let message = unsafe {
            serialize_message(JSValue::number(7.0).bits(), &[unused as usize], None).unwrap()
        };
        drop(message);
        assert!(crate::buffer::is_detached_buffer(unused as usize));
        assert_eq!(count(), before);
    }

    #[test]
    fn receiver_gc_releases_owned_backing() {
        let _guard = setup();
        let before = count();
        std::thread::spawn(move || {
            crate::gc::register_runtime_handle_root_scanner_for_tests();
            let source = js_array_buffer_new(1024 * 1024);
            let message = unsafe {
                serialize_message(
                    JSValue::pointer(source.cast()).bits(),
                    &[source as usize],
                    None,
                )
                .unwrap()
            };
            unsafe { deserialize_nanbox_on_current_thread(&message) };
            drop(message);
            collect();
            assert_eq!(
                count(),
                before,
                "full GC must drop the unrooted receiver's store"
            );
        })
        .join()
        .unwrap();
        assert_eq!(count(), before);
    }

    #[test]
    fn failed_clone_preserves_all_sources_and_bytes() {
        let _guard = setup();
        let before = count();
        let scope = RuntimeHandleScope::new();
        let source = js_array_buffer_new(64);
        let root = scope.root_raw_mut_ptr(source);
        let data = byte_address(source);
        {
            crate::buffer::js_buffer_set(source, 0, 123);
        }
        assert!(unsafe {
            serialize_message(
                JSValue::pointer(source.cast()).bits(),
                &[source as usize, source as usize],
                None,
            )
        }
        .is_err());
        let refuse = |_: u64| true;
        assert!(unsafe {
            serialize_message(
                JSValue::pointer(source.cast()).bits(),
                &[source as usize],
                Some(&refuse),
            )
        }
        .is_err());
        assert!(!crate::buffer::is_detached_buffer(source as usize));
        assert_eq!(byte_address(root.get_raw_mut_ptr::<BufferHeader>()), data);
        assert_eq!(
            crate::buffer::js_buffer_get(root.get_raw_mut_ptr::<BufferHeader>(), 0),
            123
        );
        crate::buffer::detach_array_buffer(source as usize);
        assert_eq!(count(), before);
    }

    #[test]
    fn cloned_transfer_message_has_independent_mutable_bytes() {
        let _guard = setup();
        let source = js_array_buffer_new(8);
        let message = unsafe {
            serialize_message(
                JSValue::pointer(source.cast()).bits(),
                &[source as usize],
                None,
            )
            .unwrap()
        };
        let copy = message.clone();
        let scope = RuntimeHandleScope::new();
        let a = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&message) });
        let b = scope.root_nanbox_u64(unsafe { deserialize_nanbox_on_current_thread(&copy) });
        let ap = (a.get_nanbox_u64() & POINTER_MASK) as *mut BufferHeader;
        let bp = (b.get_nanbox_u64() & POINTER_MASK) as *mut BufferHeader;
        assert_ne!(byte_address(ap), byte_address(bp));
        {
            crate::buffer::js_buffer_set(ap, 0, 55);
        }
        assert_eq!(crate::buffer::js_buffer_get(bp, 0), 0);
        crate::buffer::detach_array_buffer(ap as usize);
        crate::buffer::detach_array_buffer(bp as usize);
    }
}
