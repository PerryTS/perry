//! One declaration per catch savepoint: storage, capture, restore and a real
//! throw witness are generated together. The order here is also restore order.
//!
//! Capture providers are inlined so their HotKey accesses share the existing
//! hot TLS cache. No live subsystem state is mirrored or journaled, and the
//! cold restore path continues to use each subsystem's own cleanup rules.

use crate::gc::ShadowSavepoint;
use std::sync::atomic::{AtomicU32, Ordering};

/// Subsystems whose savepoint is skipped until they first hold state.
///
/// A `try` entry used to read every subsystem's thread-local depth, which is
/// most of what a no-throw `try` executed (a dozen thread-local reads, three of
/// them out-of-line `_tlv_get_addr` calls). Most programs never push onto most
/// of these stacks. A subsystem's bit is set, process-wide, before its first
/// push on ANY thread, so while the bit is clear every thread's state is still
/// the declared idle value and capturing that constant is exact. A restore of
/// the idle value is equally exact: it discards precisely what the protected
/// region pushed after the bit went up. Bits are never cleared.
pub(crate) mod catch_subsystem {
    /// Always captured; folded to a constant `true` test.
    pub(crate) const ALWAYS: u32 = 1 << 31;
    pub(crate) const SHADOW_FRAMES: u32 = 1 << 0;
    pub(crate) const PUMP: u32 = 1 << 1;
    pub(crate) const SET_FOREACH: u32 = 1 << 2;
    pub(crate) const MAP_FOREACH: u32 = 1 << 3;
    pub(crate) const PROTOTYPE_RESOLUTION: u32 = 1 << 4;
    pub(crate) const STATIC_PRIVATE_OWNER: u32 = 1 << 5;
    pub(crate) const PRIVATE_LEXICAL_BRAND: u32 = 1 << 6;
    pub(crate) const DERIVED_SUPER_BINDING: u32 = 1 << 7;
    pub(crate) const PRIVATE_MEMBER_ACCESS_HINTS: u32 = 1 << 8;
    // Each of these two is read only from the module that owns its stack, and
    // both modules are feature-gated (`regex/site_test.rs` behind
    // `regex-engine`, `dyn_eval` behind `dyn-eval`). Gate the bits the same
    // way, or a build without the feature fails `-D warnings` as dead code.
    #[cfg(feature = "regex-engine")]
    pub(crate) const REGEX_FACTORY: u32 = 1 << 9;
    #[cfg(feature = "dyn-eval")]
    pub(crate) const DYN_EVAL: u32 = 1 << 10;
}

static CATCH_SUBSYSTEMS_USED: AtomicU32 = AtomicU32::new(0);

/// Record that `bit`'s subsystem is about to hold non-idle state. Call it
/// before the state changes; the steady state is one load and a taken branch.
#[inline(always)]
pub(crate) fn note_catch_subsystem_used(bit: u32) {
    if CATCH_SUBSYSTEMS_USED.load(Ordering::Relaxed) & bit == 0 {
        note_catch_subsystem_used_slow(bit);
    }
}

#[cold]
#[inline(never)]
fn note_catch_subsystem_used_slow(bit: u32) {
    CATCH_SUBSYSTEMS_USED.fetch_or(bit, Ordering::SeqCst);
}

#[inline(always)]
pub(crate) fn catch_subsystem_used(bit: u32) -> bool {
    (CATCH_SUBSYSTEMS_USED.load(Ordering::Relaxed) | catch_subsystem::ALWAYS) & bit != 0
}

/// A savepoint-protected stack whose only growing operation latches its
/// subsystem bit, so no push can leave [`catch_subsystem`] stale. Reads and
/// in-place mutation go through the slice; shrinking operations need no latch.
pub(crate) struct CatchStack<T> {
    items: Vec<T>,
    bit: u32,
}

impl<T> CatchStack<T> {
    pub(crate) const fn new(bit: u32) -> Self {
        Self {
            items: Vec::new(),
            bit,
        }
    }

    #[inline]
    pub(crate) fn push(&mut self, value: T) {
        note_catch_subsystem_used(self.bit);
        self.items.push(value);
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> Option<T> {
        self.items.pop()
    }

    #[inline]
    pub(crate) fn truncate(&mut self, len: usize) {
        self.items.truncate(len);
    }

    #[cfg(test)]
    pub(crate) fn clear(&mut self) {
        self.items.clear();
    }

    #[inline]
    pub(crate) fn remove(&mut self, index: usize) -> T {
        self.items.remove(index)
    }
}

impl<T> std::ops::Deref for CatchStack<T> {
    type Target = [T];
    #[inline]
    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T> std::ops::DerefMut for CatchStack<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.items
    }
}

macro_rules! catch_savepoints {
    ($($(#[$attr:meta])* $name:ident: $ty:ty,
        capture: $capture:path, restore: $restore:path,
        latch: $latch:expr, idle: $idle:expr;)*) => {
        #[derive(Clone, Copy)]
        pub(super) struct CatchSavepoint {
            $($(#[$attr])* $name: $ty,)*
        }

        impl CatchSavepoint {
            #[inline(always)]
            pub(super) fn capture() -> Self {
                let used = CATCH_SUBSYSTEMS_USED.load(Ordering::Relaxed) | catch_subsystem::ALWAYS;
                Self {
                    $($(#[$attr])* $name: if used & $latch != 0 { $capture() } else { $idle },)*
                }
            }

            pub(super) fn restore(self) {
                $($(#[$attr])* $restore(self.$name);)*
            }
        }

        #[cfg(test)]
        mod restore_tests {
            use super::tests;
            $($(#[$attr])*
                #[test]
                fn $name() {
                    tests::assert_nested_restore($capture, $restore, tests::$name);
                }
            )*
        }

        #[cfg(test)]
        mod idle_tests {
            $($(#[$attr])*
                /// The idle constant must be exactly what a thread that never
                /// touched the subsystem captures, or skipping the read is wrong.
                #[test]
                fn $name() {
                    let (direct, idle) = std::thread::spawn(|| ($capture(), $idle))
                        .join()
                        .expect("fresh thread capture");
                    assert_eq!(direct, idle, "idle savepoint must equal a fresh thread's state");
                }
            )*
        }
    };
}

catch_savepoints! {
    // #1830, #6951: both shadow frames and expression temp roots. The frame
    // half latches inside the provider; temp roots are always read.
    shadow: ShadowSavepoint,
    capture: crate::gc::shadow_stack_savepoint,
    restore: crate::gc::shadow_stack_restore,
    latch: catch_subsystem::ALWAYS, idle: crate::gc::shadow_stack_savepoint();
    // Longjmp skips RuntimeHandleScope drops.
    runtime_handles: usize,
    capture: crate::gc::runtime_handle_stack_savepoint,
    restore: crate::gc::runtime_handle_stack_restore,
    latch: catch_subsystem::ALWAYS, idle: 0;
    // #5591: skipped method guards must not wedge the recursion counter.
    call_method: u32,
    capture: crate::object::call_method_depth_savepoint,
    restore: crate::object::call_method_depth_restore,
    latch: catch_subsystem::ALWAYS, idle: 0;
    // PR #10564 review finding: the runtime guards that displace IMPLICIT_THIS
    // around a `super()`/accessor/listener call they don't own (Temporal/Intl
    // subclass bridges, the handle-method prototype-walk accessor dispatch,
    // the stdlib listener/getter dispatchers) are a bare save/call/restore
    // pair, not `ImplicitThisScope` — neither transport runs the restore
    // statement that follows the call. The captured value is a second root
    // for the object the live cell's own scanner already protects; see
    // `scan_pending_trap_roots_mut` below.
    implicit_this: u64,
    capture: crate::object::implicit_this_trap_savepoint,
    restore: crate::object::implicit_this_trap_restore,
    latch: catch_subsystem::ALWAYS, idle: crate::value::TAG_UNDEFINED;
    // Same shape as `implicit_this`, for `new.target` (the Temporal/Intl
    // subclass `super()` bridges save/restore both together).
    new_target: u64,
    capture: crate::object::new_target_trap_savepoint,
    restore: crate::object::new_target_trap_restore,
    latch: catch_subsystem::ALWAYS, idle: crate::value::TAG_UNDEFINED;
    // Includes removal of the process-wide outer-pump contribution.
    pump: u32,
    capture: crate::stdlib_pump::pump_depth_savepoint,
    restore: crate::stdlib_pump::pump_depth_restore,
    latch: catch_subsystem::PUMP, idle: 0;
    // #9082: abandoned walks must no longer inhibit compaction.
    set_foreach: usize,
    capture: crate::set::set_foreach_stack_savepoint,
    restore: crate::set::set_foreach_stack_restore,
    latch: catch_subsystem::SET_FOREACH, idle: 0;
    map_foreach: usize,
    capture: crate::map::map_foreach_stack_savepoint,
    restore: crate::map::map_foreach_stack_restore,
    latch: catch_subsystem::MAP_FOREACH, idle: 0;
    prototype_resolution: usize,
    capture: crate::object::prototype_chain::resolution_stack_savepoint,
    restore: crate::object::prototype_chain::resolution_stack_restore,
    latch: catch_subsystem::PROTOTYPE_RESOLUTION, idle: 0;
    static_private_owner: usize,
    capture: crate::object::static_private_owner_stack_savepoint,
    restore: crate::object::static_private_owner_stack_restore,
    latch: catch_subsystem::STATIC_PRIVATE_OWNER, idle: 0;
    private_lexical_brand: usize,
    capture: crate::object::private_lexical_brand_stack_savepoint,
    restore: crate::object::private_lexical_brand_stack_restore,
    latch: catch_subsystem::PRIVATE_LEXICAL_BRAND, idle: 0;
    derived_super_binding: usize,
    capture: crate::object::derived_super_binding_stack_savepoint,
    restore: crate::object::derived_super_binding_stack_restore,
    latch: catch_subsystem::DERIVED_SUPER_BINDING, idle: 0;
    private_member_access_hints: usize,
    capture: crate::object::private_member_access_hints_savepoint,
    restore: crate::object::private_member_access_hints_restore,
    latch: catch_subsystem::PRIVATE_MEMBER_ACCESS_HINTS, idle: 0;
    #[cfg(feature = "regex-engine")]
    regex_factory: usize,
    capture: crate::regex::site_test::active_factory_stack_savepoint,
    restore: crate::regex::site_test::active_factory_stack_restore,
    latch: catch_subsystem::REGEX_FACTORY, idle: 0;
    // #6559: rooted interpreter values AND the packed call depth.
    #[cfg(feature = "dyn-eval")]
    dyn_eval: u64,
    capture: crate::dyn_eval::interp_savepoint,
    restore: crate::dyn_eval::interp_restore,
    latch: catch_subsystem::DYN_EVAL, idle: 0;
}

/// Root + rewrite `implicit_this`/`new_target` in every OPEN `try`'s captured
/// savepoint (PR #10564 review finding).
///
/// `capture()` copies the live cells' bits into this per-depth slab
/// precisely so `js_throw` can put them back after a bare save/call/restore
/// site (see `object::this_binding::implicit_this_trap_savepoint`) gets
/// longjmp'd or unwound past. That copy is a second root for the same value
/// the live cell's own scanner (`object::this_binding::
/// scan_implicit_this_roots_mut`) already protects, and it is invisible to
/// that scanner. A moving minor that runs while a `try` is open — before any
/// throw crosses it — must rewrite this copy too, or a later throw restores a
/// from-space address. Bounded by `try_depth <= MAX_TRY_DEPTH`, same as every
/// other read of this slab.
pub(super) fn scan_pending_trap_roots_mut(
    savepoints: &mut [std::mem::MaybeUninit<CatchSavepoint>],
    try_depth: usize,
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    for entry in &mut savepoints[..try_depth] {
        // SAFETY: every slot below `try_depth` was written by `capture()` in
        // `try_push_with_kind` before `try_depth` advanced past it.
        let entry = unsafe { entry.assume_init_mut() };
        visitor.visit_nanbox_u64_slot(&mut entry.implicit_this);
        visitor.visit_nanbox_u64_slot(&mut entry.new_target);
    }
}

#[cfg(test)]
mod tests;
