//! Bounded lock-free slot queue. Posts have no FIFO ordering guarantee.
//! A stalled producer owns only its own slot; consumers can drain every other
//! published slot. Scans are bounded by capacity and never wait for a producer.
use crate::sync::{AtomicUsize, Ordering, UnsafeCell};
use std::mem::MaybeUninit;
struct Slot<T> {
    state: AtomicUsize,
    value: UnsafeCell<MaybeUninit<T>>,
}
pub(crate) struct Queue<T> {
    slots: Box<[Slot<T>]>,
    enqueue: AtomicUsize,
    dequeue: AtomicUsize,
    occupied: AtomicUsize,
}
// SAFETY: state 1 belongs exclusively to a producer, state 3 to a consumer.
// Release/Acquire transfers initialized Send values between these owners.
unsafe impl<T: Send> Sync for Queue<T> {}
// SAFETY: no slot is externally borrowed; only Send values can cross threads.
unsafe impl<T: Send> Send for Queue<T> {}
/// A ring's slots, without touching them.
///
/// `Slot`'s initial value **is** the all-zero bit pattern: `state` starts at 0,
/// which is the empty state, and `value` is a `MaybeUninit` for which every
/// pattern is valid. Building the slots with `map`/`collect` writes each one,
/// which makes the entire ring resident at construction — 3.15 MiB for the
/// 32 768-slot `WorkPort` a large `max_operations` used to give a loop, in a
/// process that may never submit a blocking job.
///
/// Asking the allocator for zeroed memory instead gives a ready ring with no
/// writes at all, and a zeroed allocation this large is fresh pages the OS
/// faults in lazily. The capacity is unchanged, so every bound that depends on
/// it — including the blocking pool's "cannot overflow" credit invariant — is
/// untouched; only the pages the ring has actually used are resident.
///
/// `Queue::drop` pops until empty, and an untouched slot is state 0 (empty), so
/// no unwritten slot is ever read as a value.
#[cfg(not(loom))]
fn zeroed_slots<T>(capacity: usize) -> Box<[Slot<T>]> {
    let layout = std::alloc::Layout::array::<Slot<T>>(capacity).expect("ring layout");
    // SAFETY: `capacity >= 2` so the layout is non-zero-sized, and the all-zero
    // bit pattern is a valid `Slot<T>` as argued above.
    unsafe {
        let ptr = std::alloc::alloc_zeroed(layout).cast::<Slot<T>>();
        if ptr.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr, capacity))
    }
}

/// Under loom, `AtomicUsize` and `UnsafeCell` are instrumented types whose
/// representation is loom's business and is not all-zero, so the model build
/// keeps building each slot.
#[cfg(loom)]
fn zeroed_slots<T>(capacity: usize) -> Box<[Slot<T>]> {
    (0..capacity)
        .map(|_| Slot {
            state: AtomicUsize::new(0),
            value: UnsafeCell::new(MaybeUninit::uninit()),
        })
        .collect()
}

impl<T> Queue<T> {
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= 2 && capacity.is_power_of_two());
        Self {
            slots: zeroed_slots(capacity),
            enqueue: AtomicUsize::new(0),
            dequeue: AtomicUsize::new(0),
            occupied: AtomicUsize::new(0),
        }
    }
    pub fn push(&self, value: T) -> std::result::Result<(), T> {
        let start = self.enqueue.fetch_add(1, Ordering::Relaxed);
        for offset in 0..self.slots.len() {
            let s = &self.slots[start.wrapping_add(offset) & (self.slots.len() - 1)];
            if s.state
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                self.occupied.fetch_add(1, Ordering::Relaxed);
                s.value.with_mut(|p| {
                    // SAFETY: state 1 is held exclusively by this producer.
                    unsafe {
                        (*p).write(value);
                    }
                });
                s.state.store(2, Ordering::Release);
                return Ok(());
            }
        }
        Err(value)
    }
    pub fn pop(&self) -> Option<T> {
        if self.is_empty() {
            return None;
        }
        let start = self.dequeue.fetch_add(1, Ordering::Relaxed);
        for offset in 0..self.slots.len() {
            let s = &self.slots[start.wrapping_add(offset) & (self.slots.len() - 1)];
            if s.state
                .compare_exchange(2, 3, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                let v = s.value.with_mut(|p| {
                    // SAFETY: state 3 exclusively owns the initialized value.
                    unsafe { (*p).assume_init_read() }
                });
                self.occupied.fetch_sub(1, Ordering::Relaxed);
                s.state.store(0, Ordering::Release);
                return Some(v);
            }
        }
        None
    }
    pub fn is_empty(&self) -> bool {
        self.occupied.load(Ordering::Acquire) == 0
    }
    #[cfg(all(test, not(loom), not(target_arch = "wasm32")))]
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }
}
impl<T> Drop for Queue<T> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

#[cfg(all(test, loom))]
mod models {
    use super::*;
    use crate::sync::Arc;
    #[test]
    fn two_producers_deliver_once() {
        loom::model(|| {
            let q = Arc::new(Queue::new(2));
            let a = q.clone();
            let b = q.clone();
            let t1 = loom::thread::spawn(move || a.push(11).expect("slot"));
            let t2 = loom::thread::spawn(move || b.push(22).expect("slot"));
            t1.join().expect("producer");
            t2.join().expect("producer");
            let a = q.pop().expect("first");
            let b = q.pop().expect("second");
            assert_eq!(a + b, 33);
            assert_ne!(a, b);
            assert!(q.pop().is_none());
            q.push(44).expect("reused slot");
            assert_eq!(q.pop(), Some(44));
        });
    }
}

#[cfg(all(test, loom))]
mod publication_models {
    use super::*;
    use crate::sync::Arc;
    #[test]
    fn consumer_races_publication_and_slot_reuse() {
        loom::model(|| {
            let q = Arc::new(Queue::new(2));
            q.push(10).expect("first value");
            let producer = q.clone();
            let t = loom::thread::spawn(move || producer.push(20).expect("second value"));
            let first = q.pop();
            t.join().expect("producer");
            let mut sum = first.unwrap_or(0);
            let mut count = usize::from(first.is_some());
            while let Some(n) = q.pop() {
                sum += n;
                count += 1;
            }
            assert_eq!(sum, 30);
            assert_eq!(count, 2);
            q.push(30).expect("recycle");
            assert_eq!(q.pop(), Some(30));
            assert!(q.is_empty());
        });
    }
}

#[cfg(all(test, not(loom)))]
mod zeroed_ring {
    use super::*;

    /// A ring whose slots were never written must behave exactly like one whose
    /// slots were constructed individually.
    ///
    /// The slots come from `alloc_zeroed` so that a large ring costs only the
    /// pages it touches — `WorkPort` sized its ring from `max_operations`, which
    /// a host legitimately sets to 32 768, and writing every slot made all
    /// 3.15 MiB of it resident in a process that may never submit a blocking
    /// job. That is only sound because `Slot`'s empty state IS the all-zero bit
    /// pattern, so this checks the consequences rather than the argument:
    /// every slot is usable, the ring still refuses at exactly its capacity,
    /// and nothing unwritten is ever read back as a value.
    #[test]
    fn an_unwritten_slot_is_empty_usable_and_bounded() {
        const CAP: usize = 64;
        let q: Queue<usize> = Queue::new(CAP);
        assert!(q.is_empty(), "a freshly zeroed ring reads as empty");
        assert_eq!(q.pop(), None, "no unwritten slot may read back as a value");

        // Every slot is usable, including ones no constructor ever touched.
        for i in 0..CAP {
            q.push(i)
                .unwrap_or_else(|_| panic!("slot {i} must accept a value"));
        }
        // And the capacity still binds at exactly the declared size — the bound
        // the blocking pool's "cannot overflow" credit invariant relies on.
        assert_eq!(q.push(CAP), Err(CAP), "a full ring returns the value");

        let mut seen = vec![false; CAP];
        for _ in 0..CAP {
            let v = q.pop().expect("every pushed value comes back");
            assert!(!seen[v], "value {v} delivered twice");
            seen[v] = true;
        }
        assert!(seen.into_iter().all(|s| s), "every value was delivered");
        assert_eq!(q.pop(), None);
        assert!(q.is_empty());

        // Recycled slots still work after the ring has wrapped.
        for i in 0..CAP {
            q.push(i).expect("recycled slot");
        }
        assert_eq!(q.push(CAP), Err(CAP));
    }

    /// Dropping a ring with values still in it must drop those values and no
    /// others — an untouched zero slot must not be read as an initialised `T`.
    #[test]
    fn dropping_a_partly_filled_ring_drops_only_its_values() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static DROPPED: AtomicUsize = AtomicUsize::new(0);
        struct Counted;
        impl Drop for Counted {
            fn drop(&mut self) {
                DROPPED.fetch_add(1, Ordering::SeqCst);
            }
        }
        {
            let q: Queue<Counted> = Queue::new(1024);
            for _ in 0..3 {
                q.push(Counted).map_err(|_| ()).expect("slot");
            }
        }
        assert_eq!(
            DROPPED.load(Ordering::SeqCst),
            3,
            "exactly the three pushed values are dropped, not 1024 zero slots"
        );
    }
}
