//! I1: objects that never move once they are old.
//!
//! A per-site memo (a read site's holder entry, a method site's inherited
//! entry, their chain proofs) stores the plain address of a prototype it
//! answered from. Before this module those addresses were strong roots: each
//! site registered itself, and every collection visited and rewrote every
//! registered word. With I1 the memo needs none of that:
//!
//! * A memo may name only a **stable** object ([`stable_memo_referent`]): a
//!   `GC_TYPE_OBJECT` that carries [`OBJ_FLAG_NEVER_MOVE`] and lives in the
//!   old generation (an old-arena or longlived block). A minor never touches
//!   an old object, and the two passes that move old objects (old-page
//!   defragmentation and idle compaction, both `evacuate_selected_old_pages_
//!   collecting`) decline any block holding one ([`blocks_evacuation`]). So a
//!   stable object stays at its address for as long as it lives.
//! * Asking is what sets the bit. A young object that is asked for is refused
//!   now and promoted at its next survival (the copying minor promotes a
//!   never-move object regardless of its age, [`promotes_now`]); the site
//!   primes again after that minor. `proto_validity::mark_object_as_prototype`
//!   asks for every object that becomes a prototype, so holders are normally
//!   old after the first minor that follows their link.
//! * Liveness is not this module's business: a memo hit first compares the
//!   receiver's ShapeId, and a ShapeId pins the objects the entry names (see
//!   `object/method_site/read_holder.rs`, "Weak entries"). This module only
//!   guarantees that a live named object is still at the named address.
//!
//! The bit is set-only. Old-generation page accounting counts never-move
//! bytes as pinned bytes, so defragmentation selection skips their pages
//! instead of selecting a block it then has to decline.
use super::types::{GcHeader, GC_HEADER_SIZE, OBJ_FLAG_MEMO_NAMED, OBJ_FLAG_NEVER_MOVE};

/// Does this header word say "never moves once old"?
#[inline(always)]
pub(crate) fn header_is_never_move(obj_type: u8, reserved: u16) -> bool {
    crate::gc_type_is_object!(obj_type) && reserved & OBJ_FLAG_NEVER_MOVE != 0
}

/// [`header_is_never_move`] of a header in place.
///
/// # Safety
/// `header` points at a readable `GcHeader`.
#[inline(always)]
pub(crate) unsafe fn is_never_move(header: *const GcHeader) -> bool {
    header_is_never_move((*header).obj_type, (*header)._reserved)
}

/// The copying minor promotes this object now, whatever its age.
#[inline(always)]
pub(crate) fn promotes_now(obj_type: u8, reserved: u16) -> bool {
    #[cfg(test)]
    if sabotage::aging_ignores_never_move() {
        return false;
    }
    header_is_never_move(obj_type, reserved)
}

/// Old-page evacuation must leave this object (and so its block) in place.
///
/// # Safety
/// `header` points at a readable `GcHeader`.
#[inline(always)]
pub(crate) unsafe fn blocks_evacuation(header: *const GcHeader) -> bool {
    #[cfg(test)]
    if sabotage::evacuation_ignores_never_move() {
        return false;
    }
    is_never_move(header)
}

/// Mark `addr` never-move (set-only). An already-old object starts counting
/// as pinned on its page at once, so the next defragmentation selection does
/// not pick its block before a sweep recounts the page.
///
/// # Safety
/// `addr` is a live `GC_TYPE_OBJECT` user pointer.
pub(crate) unsafe fn request_never_move(addr: usize) {
    let header = (addr - GC_HEADER_SIZE) as *mut GcHeader;
    if crate::gc_type_is_not_object!((*header).obj_type)
        || (*header)._reserved & OBJ_FLAG_NEVER_MOVE != 0
    {
        return;
    }
    // GC_STORE_AUDIT(POINTER_FREE): a scalar header classification bit.
    (*header)._reserved |= OBJ_FLAG_NEVER_MOVE;
    if crate::arena::classify_heap_generation(addr) == crate::arena::HeapGeneration::Old {
        crate::arena::old_page_account_became_never_move(header as usize, (*header).size as usize);
    }
}

/// Is `addr` in the old generation (an old-arena or longlived block)?
///
/// # Safety
/// `addr` is a live GC user pointer.
#[cfg(test)]
pub(crate) unsafe fn lives_old(addr: usize) -> bool {
    matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Old | crate::arena::HeapGeneration::Longlived
    )
}

/// A relink displaced `addr` from its class identity. If a memo named it,
/// pin it: the memo's holder word must keep naming
/// this object for as long as the memo can be read (forever: nothing finds
/// the memo to clear it). Displacement is a once-per-class event in compiled
/// code, so this retains at most one superseded prototype per relink of a
/// prototype some site actually cached.
///
/// # Safety
/// `addr` is 0 or a live heap address.
pub(crate) unsafe fn pin_displaced_memo_prototype(addr: usize) {
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return;
    };
    if crate::gc_type_is_not_object!(header.obj_type)
        || header._reserved & OBJ_FLAG_MEMO_NAMED == 0
        || header.gc_flags & super::types::GC_FLAG_PINNED != 0
    {
        return;
    }
    #[cfg(test)]
    if sabotage::retirement_skips_pin() {
        return;
    }
    super::pin::pin_object((addr - GC_HEADER_SIZE) as *mut GcHeader);
}

/// Test-only switches that break one leg of I1, so the tests that guard it can
/// show they go red without it.
#[cfg(test)]
pub(crate) mod sabotage {
    use std::cell::Cell;

    thread_local! {
        static ADMIT_YOUNG: Cell<bool> = const { Cell::new(false) };
        static EVACUATE_NEVER_MOVE: Cell<bool> = const { Cell::new(false) };
        static AGE_NEVER_MOVE: Cell<bool> = const { Cell::new(false) };
        static SKIP_RETIRE_PIN: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn retirement_skips_pin() -> bool {
        SKIP_RETIRE_PIN.with(Cell::get)
    }

    pub(crate) fn evacuation_ignores_never_move() -> bool {
        EVACUATE_NEVER_MOVE.with(Cell::get)
    }
    pub(crate) fn aging_ignores_never_move() -> bool {
        AGE_NEVER_MOVE.with(Cell::get)
    }
}

/// Test-only: unit tests of memo LOGIC publish synthetic or young holders
/// inside a no-move scope, where nothing can move them. They opt out of the
/// publication rule with this guard; the GC-facing tests never do.
#[cfg(test)]
pub(crate) mod test_admit {
    use std::cell::Cell;

    thread_local! {
        static ON: Cell<bool> = const { Cell::new(false) };
    }

    /// Admit every referent until dropped.
    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn new() -> Self {
            Guard(ON.with(|c| c.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            ON.with(|c| c.set(self.0));
        }
    }
}
