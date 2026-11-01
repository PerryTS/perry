//! Per-object meta-record accessors and the cell-generic `meta` edge (#8891).
//!
//! Split out of `object/mod.rs` for the 2,000-line file gate.

use super::*;

/// Why a meta record is being created: one variant per row of the A1 design's
/// reason table. A new call site needs a reason, and a new reason needs its
/// excluding fact in [`MetaReason::excluded_by`] (matched without a wildcard),
/// or it is `RuntimeBornOnly`, which debug builds check at the funnel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetaReason {
    Spill,
    SpillReserve,
    Overflow,
    PrototypeOnReceiver,
    PrototypeHolder,
    BecomesPrototype,
    EvalBrand,
    AdoptFamilyLayout,
    ClassEvaluationPrivateStorage,
    CtorFnPrototype,
    BuiltinSeal,
    RuntimePrototypeName,
    HttpPrototypes,
    ReflectMetadata,
    BoxedPrimitive,
    SetKeyIdentity,
    WeakStorage,
    OwnedState,
    NativePayload,
    NativeThisAlias,
    WasmWrapper,
    RuntimeHandle,
    ArraySubclassElements,
    ArrayIteratorBrand,
    ClosureProps,
    CellExpando,
}

/// The whole-program fact (design section 2.1) that keeps a meta-less family
/// away from a reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExcludingFact {
    /// No token of the family reaches top (escape, unmodeled builtins, vm, N-API).
    P1,
    /// No hard mark (defineProperty, setPrototypeOf, delete, freeze, Proxy, ...).
    P2,
    /// Fixed layout: every key ever stored is a slotted birth key.
    P3,
    /// No native parent and no `extends`.
    P4,
    /// The family is never used as a prototype.
    P5,
    /// The family is never a collection element.
    P6,
    /// The family is never a constructor-return override.
    // No reason is excluded by P7 today; the numbering is the design's.
    #[allow(dead_code)]
    P7,
    /// The family's class has no evaluation brand.
    P8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exclusion {
    /// Only objects the runtime allocated itself reach this reason.
    RuntimeBornOnly,
    Fact(ExcludingFact),
}

impl MetaReason {
    pub(crate) const fn excluded_by(self) -> Exclusion {
        use ExcludingFact::*;
        use Exclusion::{Fact, RuntimeBornOnly};
        match self {
            MetaReason::Spill => Fact(P3),
            MetaReason::SpillReserve => RuntimeBornOnly,
            MetaReason::Overflow => Fact(P3),
            MetaReason::PrototypeOnReceiver => Fact(P2),
            MetaReason::PrototypeHolder => RuntimeBornOnly,
            MetaReason::BecomesPrototype => Fact(P5),
            MetaReason::EvalBrand => Fact(P8),
            MetaReason::AdoptFamilyLayout => RuntimeBornOnly,
            MetaReason::ClassEvaluationPrivateStorage => RuntimeBornOnly,
            MetaReason::CtorFnPrototype => RuntimeBornOnly,
            MetaReason::BuiltinSeal => RuntimeBornOnly,
            MetaReason::RuntimePrototypeName => RuntimeBornOnly,
            MetaReason::HttpPrototypes => RuntimeBornOnly,
            MetaReason::ReflectMetadata => Fact(P1),
            MetaReason::BoxedPrimitive => RuntimeBornOnly,
            MetaReason::SetKeyIdentity => Fact(P6),
            MetaReason::WeakStorage => RuntimeBornOnly,
            MetaReason::OwnedState => Fact(P1),
            MetaReason::NativePayload => Fact(P4),
            MetaReason::NativeThisAlias => Fact(P4),
            MetaReason::WasmWrapper => Fact(P1),
            MetaReason::RuntimeHandle => RuntimeBornOnly,
            MetaReason::ArraySubclassElements => Fact(P4),
            MetaReason::ArrayIteratorBrand => RuntimeBornOnly,
            MetaReason::ClosureProps => RuntimeBornOnly,
            MetaReason::CellExpando => RuntimeBornOnly,
        }
    }
}

/// `reason` documents the caller and, in debug builds, checks that a
/// `RuntimeBornOnly` reason really got a runtime-born (meta-form) object.
#[inline(always)]
fn debug_check_reason(obj_type: impl FnOnce() -> u8, reason: MetaReason) {
    if cfg!(debug_assertions) && reason.excluded_by() == Exclusion::RuntimeBornOnly {
        assert_eq!(obj_type(), crate::gc::GC_TYPE_OBJECT, "{reason:?}");
    }
}

pub(crate) unsafe fn object_meta_ensure_for_cell(
    user_ptr: usize,
    reason: MetaReason,
) -> Option<*mut ObjectMeta> {
    let _ = reason;
    let slot = cell_meta_slot(user_ptr)?;
    if !(*slot).is_null() {
        return Some(*slot);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner = scope.root_raw_mut_ptr(user_ptr as *mut u8);
    let meta = arena_alloc_gc(
        std::mem::size_of::<ObjectMeta>(),
        8,
        crate::gc::GC_TYPE_OBJECT_META,
    ) as *mut ObjectMeta;
    let user_ptr = owner.get_raw_mut_ptr::<u8>() as usize;
    let slot = cell_meta_slot(user_ptr)?;
    if !(*slot).is_null() {
        // A re-entrant path installed one while we allocated; keep it.
        return Some(*slot);
    }
    (*meta).prototype = 0;
    (*meta).boxed_primitive = 0;
    (*meta).reflect_metadata = 0;
    (*meta).key_identity = 0;
    (*meta).owned_state = 0;
    (*meta).flags = 0;
    (*meta).spill = 0;
    (*meta).private_evaluation_brand = 0;
    (*meta).proto_serial = 0;
    (*meta).native_state = 0;
    (*meta).function_layout_birth = 0;
    #[cfg(feature = "shape-mint-diag")]
    crate::object::shape_mint_census::note_event("ObjectMeta record born");
    (*meta).array_tail_object_hot = 0;
    (*meta).array_subclass_dense_key = 0;
    (*meta).array_subclass_dense_slots = 0;
    (*meta).array_subclass_dense_bounds = 0;
    (*meta).expando = 0;
    (*meta).elements = 0;
    (*meta).overflow = 0;
    (*meta).arguments = 0;
    (*meta).instance_birth = 0;
    // GC_STORE_AUDIT(BARRIERED): header-slot store followed by an object-slot
    // barrier, exactly as `object_meta_ensure` does for an `ObjectHeader`.
    // The reloaded cell is live; consume its current generation proof.
    // Young arena holders need only the active incremental-mark barrier.
    *slot = meta;
    crate::gc::runtime_write_barrier_resolved_slot(user_ptr, slot as usize, meta as u64);
    Some(meta)
}

pub(crate) unsafe fn object_meta_ensure(obj: MetaObj, reason: MetaReason) -> *mut ObjectMeta {
    debug_check_reason(|| object_type(obj.as_ptr()), reason);
    let obj = obj.as_ptr();
    if !(*obj).meta.is_null() {
        return (*obj).meta;
    }
    // Root the owner across the allocation: `arena_alloc_gc` can trigger a
    // copied-minor that MOVES `obj`, and the header store below must land
    // in the live copy, not the stale from-space one. Reload through the
    // handle after the allocation. (The fresh `meta` record itself cannot
    // move before the store — no allocation happens in between.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let meta = arena_alloc_gc(
        std::mem::size_of::<ObjectMeta>(),
        8,
        crate::gc::GC_TYPE_OBJECT_META,
    ) as *mut ObjectMeta;
    let obj = obj_handle.get_raw_mut_ptr::<ObjectHeader>();
    if !(*obj).meta.is_null() {
        // A GC-triggered re-entrant path installed one meanwhile; keep it
        // (the fresh record above is unreferenced and dies with the cycle).
        return (*obj).meta;
    }
    (*meta).prototype = 0;
    (*meta).boxed_primitive = 0;
    (*meta).reflect_metadata = 0;
    (*meta).key_identity = 0;
    (*meta).owned_state = 0;
    (*meta).flags = 0;
    (*meta).spill = 0;
    (*meta).private_evaluation_brand = 0;
    (*meta).proto_serial = 0;
    (*meta).native_state = 0;
    (*meta).function_layout_birth = 0;
    #[cfg(feature = "shape-mint-diag")]
    crate::object::shape_mint_census::note_event("ObjectMeta record born");
    (*meta).array_tail_object_hot = 0;
    (*meta).array_subclass_dense_key = 0;
    (*meta).array_subclass_dense_slots = 0;
    (*meta).array_subclass_dense_bounds = 0;
    (*meta).expando = 0;
    (*meta).elements = 0;
    (*meta).overflow = 0;
    (*meta).arguments = 0;
    (*meta).instance_birth = 0;
    // GC_STORE_AUDIT(BARRIERED): meta-record edge is a header-slot store
    // followed by an object-slot barrier, mirroring `set_object_keys_array`.
    // Reloaded after the allocation: promotion, if any, is reflected in the
    // live header. No safepoint intervenes before the resolved-slot barrier.
    (*obj).meta = meta;
    crate::gc::runtime_write_barrier_resolved_slot(
        obj as usize,
        &(*obj).meta as *const _ as usize,
        meta as u64,
    );
    meta
}

/// GC slot accessor for the `meta` header edge (#6759 Phase B): a raw-pointer
/// child slot. The GC type table calls this
/// only for `GC_TYPE_OBJECT`; RegExp uses its dedicated slot descriptor.
pub(crate) unsafe fn gc_object_meta_slot(user_ptr: usize) -> Option<*mut u64> {
    if user_ptr == 0 {
        return None;
    }
    let obj = user_ptr as *mut ObjectHeader;
    if (*obj).meta.is_null() {
        return None;
    }
    Some(&mut (*obj).meta as *mut _ as *mut u64)
}

/// The fail-closed abort for a meta-less family object that reached a meta
/// reason (A1 design 2.4). It sits on the cold side of a caller's existing
/// type dispatch; the first callers arrive with the meta-less births.
///
/// # Safety
/// `obj` is the user pointer of a live ordinary object.
#[cold]
#[inline(never)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) unsafe fn a1_internal_error(obj: *const ObjectHeader, reason: MetaReason) -> ! {
    let family = layout_desc::family_desc_of(obj).map_or(0, |desc| desc.family);
    eprintln!(
        "perry internal error A1: meta-less family {family:#x} (class {}) reached {reason:?}",
        (*obj).class_id
    );
    std::process::abort();
}
