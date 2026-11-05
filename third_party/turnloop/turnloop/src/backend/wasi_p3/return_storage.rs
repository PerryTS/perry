//! Canonical lists may arrive on any host-progress boundary, including another
//! loop's wait. Share retained slots across live p3 backends, and return each slot
//! only after its owning operation consumes or cancels the result. Never reset an
//! arena at the end of a turn. Debug uses the std allocator: see docs/wasm.md.
use std::{ops::Deref, rc::Rc};

#[cfg(not(debug_assertions))]
mod retained {
    use std::{
        alloc::{Layout, alloc, handle_alloc_error, realloc},
        cell::{Cell, RefCell},
        ptr,
        rc::{Rc, Weak},
        sync::atomic::{AtomicPtr, Ordering},
    };
    const DATAGRAM_CAPACITY: usize = 65536;
    #[derive(Debug)]
    struct Slot {
        bytes: Vec<u8>,
        busy: bool,
    }
    #[derive(Default, Debug)]
    pub struct Arena {
        slots: RefCell<Vec<Slot>>,
        reservations: Cell<usize>,
    }
    thread_local! {
        static SHARED: RefCell<Weak<Arena>> = const { RefCell::new(Weak::new()) };
    }
    // A p3 component has one guest agent. Atomics avoid static-mut references
    // and work before std TLS startup. No arbitrary guest callback runs in scope.
    static ACTIVE: AtomicPtr<Arena> = AtomicPtr::new(ptr::null_mut());
    impl Arena {
        pub fn shared() -> Rc<Self> {
            SHARED.with(|shared| {
                if let Some(arena) = shared.borrow().upgrade() {
                    return arena;
                }
                let arena = Rc::new(Self::default());
                *shared.borrow_mut() = Rc::downgrade(&arena);
                arena
            })
        }
        pub fn reserve(self: &Rc<Self>) -> Reservation {
            let n = self.reservations.get() + 1;
            self.reservations.set(n);
            let mut slots = self.slots.borrow_mut();
            if slots.len() < n {
                slots.push(Slot {
                    bytes: vec![0; DATAGRAM_CAPACITY],
                    busy: false,
                });
            }
            Reservation(self.clone())
        }
        pub fn release(&self, index: usize) {
            let mut slots = self.slots.borrow_mut();
            assert!(slots[index].busy, "canonical slot released twice");
            slots[index].busy = false;
        }
        pub fn locate(&self, ptr: *const u8, len: usize) -> Option<usize> {
            self.slots.borrow().iter().position(|slot| {
                if slot.bytes.as_ptr() != ptr {
                    return false;
                }
                assert!(slot.busy && len <= slot.bytes.len());
                true
            })
        }
    }
    #[derive(Debug)]
    pub struct Reservation(Rc<Arena>);
    impl Drop for Reservation {
        fn drop(&mut self) {
            self.0.reservations.set(self.0.reservations.get() - 1);
        }
    }
    struct Reset(*mut Arena);
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.store(self.0, Ordering::Relaxed);
        }
    }
    pub fn scoped<T>(f: impl FnOnce() -> T) -> T {
        let arena = SHARED.with(|shared| shared.borrow().upgrade());
        let pointer = arena
            .as_ref()
            .map_or(ptr::null_mut(), |a| Rc::as_ptr(a).cast_mut());
        let _reset = Reset(ACTIVE.swap(pointer, Ordering::Relaxed));
        f()
    }
    // SAFETY: overrides std's weak canonical ABI export with the identical ABI.
    // Only hand-lowered async imports opt in. Their lists/errors use ReturnBytes,
    // never generated Vec/String destruction. All other imports use Rust's heap.
    #[unsafe(no_mangle)]
    unsafe extern "C" fn cabi_realloc(
        old: *mut u8,
        old_len: usize,
        align: usize,
        len: usize,
    ) -> *mut u8 {
        if len == 0 {
            return ptr::without_provenance_mut(align);
        }
        let arena = ACTIVE.load(Ordering::Relaxed);
        if !arena.is_null() && old_len != 0 {
            // SAFETY: active scope retains the arena; old describes a canonical allocation.
            let arena = unsafe { &*arena };
            if let Some(index) = arena.locate(old, old_len) {
                assert_eq!(align, 1);
                if len <= DATAGRAM_CAPACITY {
                    return old;
                }
                let layout = Layout::from_size_align(len, align).expect("canonical layout");
                // SAFETY: allocate a new heap list before releasing retained storage.
                let new = unsafe { alloc(layout) };
                if new.is_null() {
                    handle_alloc_error(layout);
                }
                // SAFETY: old has old_len initialized bytes and new has len capacity.
                unsafe { ptr::copy_nonoverlapping(old, new, old_len.min(len)) };
                arena.release(index);
                return new;
            }
        }
        if !arena.is_null() && old_len == 0 && align == 1 && len <= DATAGRAM_CAPACITY {
            // SAFETY: scoped holds an Rc to the arena until this import returns.
            let arena = unsafe { &*arena };
            for slot in arena.slots.borrow_mut().iter_mut() {
                if !slot.busy {
                    slot.busy = true;
                    return slot.bytes.as_mut_ptr();
                }
            }
        }
        // Larger exceptional error text (or imports outside the scope) retains
        // ordinary owned allocation. UDP steady state has one slot per socket.
        let layout = Layout::from_size_align(if old_len == 0 { len } else { old_len }, align)
            .expect("canonical layout");
        // SAFETY: non-arena pointers and original layouts follow canonical realloc.
        let result = unsafe {
            if old_len == 0 {
                alloc(layout)
            } else {
                realloc(old, layout, len)
            }
        };
        if result.is_null() {
            handle_alloc_error(layout);
        }
        result
    }
}
#[cfg(not(debug_assertions))]
pub use retained::{Arena, Reservation, scoped};

#[cfg(debug_assertions)]
pub struct Arena;
#[cfg(debug_assertions)]
#[derive(Debug)]
pub struct Reservation;
#[cfg(debug_assertions)]
impl Arena {
    pub fn shared() -> Rc<Self> {
        Rc::new(Self)
    }
    pub fn reserve(self: &Rc<Self>) -> Reservation {
        Reservation
    }
}
#[cfg(debug_assertions)]
pub fn scoped<T>(f: impl FnOnce() -> T) -> T {
    f()
}

pub enum ReturnBytes {
    Heap(Vec<u8>),
    #[cfg(not(debug_assertions))]
    Retained {
        arena: Rc<Arena>,
        index: usize,
        ptr: *const u8,
        len: usize,
    },
}
impl ReturnBytes {
    /// Own the canonical list, including its release obligation.
    ///
    /// # Safety
    /// Transfer an initialized canonical byte list exactly once. A nonempty
    /// list must belong to a live retained arena or the canonical heap allocator,
    /// with capacity equal to len for heap storage. No host task may still write it.
    pub unsafe fn take(ptr: u32, len: usize) -> Self {
        if len == 0 {
            return Self::Heap(Vec::new());
        }
        #[cfg(not(debug_assertions))]
        {
            let arena = Arena::shared();
            if let Some(index) = arena.locate(ptr as *const u8, len) {
                return Self::Retained {
                    arena,
                    index,
                    ptr: ptr as *const u8,
                    len,
                };
            }
        }
        // SAFETY: caller transfers a nonempty canonical heap list, capacity len.
        Self::Heap(unsafe { Vec::from_raw_parts(ptr as *mut u8, len, len) })
    }
}
impl Deref for ReturnBytes {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Heap(bytes) => bytes,
            #[cfg(not(debug_assertions))]
            Self::Retained { ptr, len, .. } => {
                // SAFETY: the owned busy slot stays allocated and cannot be
                // reused until Drop; the canonical host initialized len bytes.
                unsafe { std::slice::from_raw_parts(*ptr, *len) }
            }
        }
    }
}
impl Drop for ReturnBytes {
    fn drop(&mut self) {
        #[cfg(not(debug_assertions))]
        if let Self::Retained { arena, index, .. } = self {
            arena.release(*index);
        }
    }
}

#[cfg(all(test, not(debug_assertions)))]
mod tests {
    use super::*;
    #[test]
    fn pending_lists_keep_distinct_storage_across_scopes_and_owners() {
        unsafe extern "C" {
            fn cabi_realloc(old: *mut u8, old_len: usize, align: usize, len: usize) -> *mut u8;
        }
        let first = Arena::shared();
        let second = Arena::shared();
        assert!(Rc::ptr_eq(&first, &second));
        let reservation_a = first.reserve();
        let reservation_b = second.reserve();
        let a = scoped(|| {
            // SAFETY: canonical fresh allocation, supported byte alignment/length.
            unsafe { cabi_realloc(std::ptr::null_mut(), 0, 1, 65536) }
        });
        let b = scoped(|| {
            // SAFETY: second pending allocation must not overwrite a.
            unsafe { cabi_realloc(std::ptr::null_mut(), 0, 1, 65536) }
        });
        assert_ne!(a, b);
        // SAFETY: each exclusive allocation has capacity 65536. Simulate the
        // canonical host initializing both lists before their return events.
        unsafe {
            a.write_bytes(0x31, 65536);
            b.write_bytes(0x92, 65536);
        }
        drop(reservation_a);
        drop(first);
        // SAFETY: both initialized canonical lists now transfer to their owner.
        let (a_list, b_list) = unsafe {
            (
                ReturnBytes::take(a as u32, 65536),
                ReturnBytes::take(b as u32, 65536),
            )
        };
        assert!(a_list.iter().all(|&x| x == 0x31));
        assert!(b_list.iter().all(|&x| x == 0x92));
        drop(a_list); // The cancellation path discards a returned list the same way.
        let reused = scoped(|| {
            // SAFETY: allocate after a's return; b remains busy and immutable.
            unsafe { cabi_realloc(std::ptr::null_mut(), 0, 1, 17) }
        });
        assert_eq!(reused, a);
        assert!(b_list.iter().all(|&x| x == 0x92));
        // SAFETY: canonical initialization then ownership transfer of 17 bytes.
        unsafe {
            reused.write_bytes(0x74, 17);
            drop(ReturnBytes::take(reused as u32, 17));
        }
        drop(b_list);
        drop(reservation_b);
        assert_eq!(Rc::strong_count(&second), 1);
    }
    #[test]
    fn error_strings_and_exceptional_growth_return_owned_storage() {
        unsafe extern "C" {
            fn cabi_realloc(old: *mut u8, old_len: usize, align: usize, len: usize) -> *mut u8;
        }
        let arena = Arena::shared();
        let _reservation = arena.reserve();
        let ptr = scoped(|| {
            // SAFETY: fresh canonical byte allocation, valid layout.
            unsafe { cabi_realloc(std::ptr::null_mut(), 0, 1, 17) }
        });
        // SAFETY: initialize the full list as canonical error text.
        unsafe { ptr.write_bytes(b'x', 17) };
        let mut error = [1, 14, 1, ptr as u32, 17];
        assert_eq!(
            super::super::socket_result(&mut error)
                .expect_err("Other string")
                .kind,
            crate::ErrorKind::Other
        );
        let reused = scoped(|| {
            // SAFETY: a fresh list after error consumption must reuse its slot.
            unsafe { cabi_realloc(std::ptr::null_mut(), 0, 1, 17) }
        });
        assert_eq!(reused, ptr);
        // SAFETY: initialize before canonical realloc copies the old prefix.
        unsafe { reused.write_bytes(0x58, 17) };
        let grown = scoped(|| {
            // SAFETY: canonical growth with the allocation's actual original size.
            unsafe { cabi_realloc(reused, 17, 1, 65537) }
        });
        assert_ne!(grown, reused);
        // SAFETY: growth initialized only the original prefix; initialize the tail
        // before creating a byte slice and taking ownership of the whole list.
        let bytes = unsafe {
            grown.add(17).write_bytes(0x39, 65537 - 17);
            ReturnBytes::take(grown as u32, 65537)
        };
        assert!(bytes[..17].iter().all(|&x| x == 0x58));
        assert!(bytes[17..].iter().all(|&x| x == 0x39));
        let mut fixed_error = [1, 9, 0, 0, 0];
        assert_eq!(
            super::super::socket_result(&mut fixed_error)
                .expect_err("refused")
                .kind,
            crate::ErrorKind::ConnectionRefused
        );
        // SAFETY: empty canonical lists may legally use a null pointer.
        assert!(unsafe { ReturnBytes::take(0, 0) }.is_empty());
    }
}
