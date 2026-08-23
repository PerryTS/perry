//! #8259 — GC custody for the pump's in-flight listener dispatch.
//!
//! Every arm of `js_ext_net_drain_pending` used to snapshot its listeners
//! into a bare `Vec<i64>` (`listeners_for`) and then, for some arms, allocate
//! a payload (`alloc_buffer`/`alloc_string`/`build_error_object`) before
//! calling each closure. Both steps can collect: the allocation directly, and
//! every `call*` by running arbitrary user JS (the #8259 witness's `once`
//! handler churns 30k objects). Under the evacuating arms the collection
//! MOVES the closures; `scan_net_roots` rewrites the canonical
//! `statics::listeners()` slots, but a bare local snapshot is invisible to
//! it, so the *next* callback in the loop — and the already-built payload —
//! were dereferenced at their OLD addresses. Deterministic SIGSEGV under
//! `force_verify` (`test_gap_gc_net_once_flags_rekey`, exit 139).
//!
//! Two arms were worse: `ServerListening` / `ServerClose` REMOVE their
//! callbacks from the table before firing (one-shot semantics), so during
//! dispatch nothing rooted them at all — a full sweep could free, not just
//! move, them.
//!
//! Same custody pattern as perry-ext-http's `H2_DRAINED_EVENTS` /
//! `H2_ACTIVE_CALLBACKS` (#8216): park the snapshot (and at most one
//! NaN-boxed payload per frame) in scanned thread-locals, re-read each slot
//! immediately before use so the copying GC's rewrite is observed, and pop
//! on drop. Frames are strictly nested (a re-entrant pump inside `call1`
//! builds and drops its own frame before the outer loop resumes), so plain
//! stacks suffice.

use perry_ffi::GcRootVisitor;
use std::cell::RefCell;

thread_local! {
    /// Callback addresses of every dispatch frame currently on the stack.
    /// Raw closure addresses (same representation as `statics::listeners()`),
    /// visited and REWRITTEN by [`scan`].
    static CBS: RefCell<Vec<i64>> = const { RefCell::new(Vec::new()) };
    /// At most one NaN-boxed payload per frame that carries one (Data's
    /// buffer/string, the error objects, ServerDrop's info object).
    /// Immediate payloads (booleans, handle-tagged ids) are not parked —
    /// they hold no heap address.
    static PAYLOADS: RefCell<Vec<u64>> = const { RefCell::new(Vec::new()) };
}

/// Visit every parked slot. Wired into `scan_net_roots` so the registered
/// scanner keeps custody of in-flight dispatches; the census call-graph walk
/// (scripts/gc_runtime_root_holders.py) certifies the two holders through
/// this path.
pub(crate) fn scan(visitor: &mut GcRootVisitor<'_>) {
    CBS.with(|cbs| {
        for cb in cbs.borrow_mut().iter_mut() {
            if *cb != 0 {
                visitor.visit_i64_slot(cb);
            }
        }
    });
    PAYLOADS.with(|payloads| {
        for bits in payloads.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(bits);
        }
    });
}

/// One parked snapshot of listener callbacks (+ optionally one payload).
/// Construct with [`DispatchFrame::park`] BEFORE any allocating payload
/// prep; read every callback via [`DispatchFrame::cb`] and the payload via
/// [`DispatchFrame::payload_bits`] immediately before each call.
pub(crate) struct DispatchFrame {
    cb_base: usize,
    cb_len: usize,
    payload_base: Option<usize>,
    payload_len: usize,
}

impl DispatchFrame {
    pub(crate) fn park(cbs: Vec<i64>) -> Self {
        let cb_len = cbs.len();
        let cb_base = CBS.with(|s| {
            let mut s = s.borrow_mut();
            let base = s.len();
            s.extend_from_slice(&cbs);
            base
        });
        DispatchFrame {
            cb_base,
            cb_len,
            payload_base: None,
            payload_len: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.cb_len == 0
    }

    pub(crate) fn len(&self) -> usize {
        self.cb_len
    }

    /// Fresh read: the scanner may have rewritten the slot since parking.
    pub(crate) fn cb(&self, i: usize) -> i64 {
        debug_assert!(i < self.cb_len);
        CBS.with(|s| s.borrow()[self.cb_base + i])
    }

    /// Park the frame's NaN-boxed payload. Call at most once, AFTER `park`
    /// (so the callbacks were already in custody while the payload was
    /// allocated) and BEFORE the first `call*`.
    pub(crate) fn set_payload(&mut self, bits: u64) {
        self.set_payloads(&[bits]);
    }

    /// Park several NaN-boxed arguments for a single synchronous dispatch.
    pub(crate) fn set_payloads(&mut self, bits: &[u64]) {
        debug_assert!(self.payload_base.is_none());
        if bits.is_empty() {
            return;
        }
        let base = PAYLOADS.with(|p| {
            let mut p = p.borrow_mut();
            let base = p.len();
            p.extend_from_slice(bits);
            base
        });
        self.payload_base = Some(base);
        self.payload_len = bits.len();
    }

    /// Fresh read of the parked payload (rewritten if its object moved).
    pub(crate) fn payload_bits(&self) -> u64 {
        self.payload_bits_at(0)
    }

    /// Fresh read of one parked argument.
    pub(crate) fn payload_bits_at(&self, index: usize) -> u64 {
        debug_assert!(index < self.payload_len);
        let base = self
            .payload_base
            .expect("payload_bits_at without set_payloads");
        PAYLOADS.with(|p| p.borrow()[base + index])
    }
}

impl Drop for DispatchFrame {
    fn drop(&mut self) {
        CBS.with(|s| {
            let mut s = s.borrow_mut();
            debug_assert_eq!(s.len(), self.cb_base + self.cb_len, "unnested frame drop");
            s.truncate(self.cb_base);
        });
        if let Some(base) = self.payload_base {
            PAYLOADS.with(|p| {
                let mut p = p.borrow_mut();
                debug_assert_eq!(p.len(), base + self.payload_len, "unnested payload drop");
                p.truncate(base);
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_nest_and_pop() {
        let outer = DispatchFrame::park(vec![11, 22]);
        assert_eq!(outer.len(), 2);
        assert_eq!(outer.cb(0), 11);
        {
            let mut inner = DispatchFrame::park(vec![33]);
            inner.set_payload(0x7FFD_0000_0000_1234);
            assert_eq!(inner.cb(0), 33);
            assert_eq!(inner.payload_bits(), 0x7FFD_0000_0000_1234);
        }
        // Inner frame popped; outer slots intact.
        assert_eq!(outer.cb(1), 22);
        CBS.with(|s| assert_eq!(s.borrow().len(), 2));
        PAYLOADS.with(|p| assert!(p.borrow().is_empty()));
        drop(outer);
        CBS.with(|s| assert!(s.borrow().is_empty()));
    }

    #[test]
    fn parked_slots_are_the_read_path() {
        // A rewrite of the thread-local slot (what the GC scanner does when
        // an object moves) must be visible through the frame's accessors —
        // that is the entire point of parking.
        let mut frame = DispatchFrame::park(vec![0x1000]);
        frame.set_payloads(&[0x7FFF_0000_0000_2000, 0x7FFF_0000_0000_3000]);
        CBS.with(|s| s.borrow_mut()[0] = 0x9000);
        PAYLOADS.with(|p| p.borrow_mut()[0] = 0x7FFF_0000_0000_A000);
        assert_eq!(frame.cb(0), 0x9000);
        assert_eq!(frame.payload_bits(), 0x7FFF_0000_0000_A000);
        assert_eq!(frame.payload_bits_at(1), 0x7FFF_0000_0000_3000);
    }

    #[test]
    fn empty_frame_is_cheap_and_safe() {
        let frame = DispatchFrame::park(Vec::new());
        assert!(frame.is_empty());
        drop(frame);
        CBS.with(|s| assert!(s.borrow().is_empty()));
    }
}
