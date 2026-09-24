//! Observable `[[Prototype]]` for ordinary heap objects (#2820, #6759 B).
//!
//! Perry bakes class IDs at allocation time, so it cannot rewrite an object's
//! baked prototype chain. But `Object.setPrototypeOf(obj, proto)` on an
//! *ordinary* object (a `{}` literal, an `Object.create(...)` result, etc.)
//! must be observable: a later `Object.getPrototypeOf(obj)` returns the same
//! `proto`, and an inherited property read (`obj.x` where `x` lives on `proto`)
//! walks to it.
//!
//! Storage is split by owner kind (#6759 Phase B):
//!
//! * A genuine shaped `GC_TYPE_OBJECT` stores the recorded bits in its own
//!   per-object [`crate::object::ObjectMeta`] record, reached from the
//!   object header in two dependent loads — no mutex, no address-keyed
//!   probe, and structurally immune to the stale-address-reuse hazard (the
//!   record lives and dies with its owner; GC traces/rewrites it through
//!   the ordinary Object descriptor).
//! * Every other owner kind — real/lazy arrays, typed arrays, native
//!   handle-band ids, proxy ids — keeps the RESIDUAL address-keyed registry
//!   below, with its original GC hooks (scanner, owner-move rekey,
//!   dead-owner prune). Migrating arrays needs an `ArrayHeader` slot and is
//!   a later #6759 tranche.
//!
//! `proto_bits` for an explicit `Object.setPrototypeOf(obj, null)` is
//! `TAG_NULL`, so a recorded-null entry is distinguishable from "no entry
//! recorded" (default prototype); in the meta record, 0 means unset.
//!
//! # What the residual registry costs an inherited read: nothing (measured)
//!
//! The 2026-09-20 object-model design note proposed deleting `OBJECT_PROTOTYPES`
//! on the theory that its `Mutex` plus a hash probe per chain level was "likely
//! part of the 950 ns inherited read". Callgrind says otherwise. On
//! `const P={a:1}; const O=Object.create(P); O.a` at 200 k reads (v0.5.1619,
//! `--debug-symbols`, x86-64-v3), ~1300 instructions per inherited read split
//! as: `native_get::try_data_get_bytes` self 344; `class_prototype_object` 192,
//! of which 118 is the SipHash of the `CLASS_PROTOTYPE_OBJECTS` probe;
//! `keys_find_slot_by_bytes_resolved` 180 (twice — receiver, then holder);
//! `get_field_ic_miss_impl` self 153; `closure_dynamic_prop_by_key` 90;
//! `shape_descriptor_by_id` 70; `is_anon_shape_class_id` 54;
//! `class_decl_prototype_object` 41; `from_utf8` 38; `is_arguments_object` 28.
//! Neither `get_object_prototypes` nor `pthread_mutex_lock` appears at all,
//! because #6759 phase B already took every `GC_TYPE_OBJECT` off this table —
//! the prototype of an `Object.create` receiver comes from the class registry,
//! not from here.
//!
//! So the table is not a duplicate of `ObjectMeta.prototype` waiting to be
//! deleted; it is the ONLY storage the kinds below have. Removing it means
//! giving each of them a prototype slot of its own, kind by kind: arrays and
//! lazy arrays (an `ArrayHeader` slot — the #6759 tranche this module's header
//! already names), typed arrays, `ArrayBuffer`/`SharedArrayBuffer`/`DataView`
//! (`BufferHeader`), `GC_TYPE_REGEXP`, `Map`/`Set`/`Error`/`Promise`/`Date`/
//! `Temporal` cells, closures (`dyn_eval`), and native handle-band ids, which
//! are integers with no cell at all (the Express `res`/`req` case in
//! `object_ops::define_properties`) and so need a different answer entirely.
//! None of that work makes an inherited property read faster.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Set when `Object.setPrototypeOf` has retargeted a REAL ARRAY's
/// [[Prototype]] anywhere in the program. The typed-feedback array guards
/// consult it (one relaxed load) so the inline raw-slot fast path stands
/// down: holes/OOB reads must then walk the custom chain (test262
/// copyWithin/coerced-values-start-change-*).
static ARRAY_TARGET_PROTO_RECORDED: AtomicBool = AtomicBool::new(false);

pub(crate) fn array_static_proto_recorded() -> bool {
    ARRAY_TARGET_PROTO_RECORDED.load(Ordering::Relaxed)
}

const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

static OBJECT_PROTOTYPES: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();
crate::perry_thread_local! {
    /// Owners currently walking a recorded prototype chain. Although
    /// `Object.setPrototypeOf` normally rejects cycles, residual/native owners
    /// and custom-construction links can still expose a malformed chain. Keep
    /// recursive property lookup bounded and stop on a repeated owner. These
    /// are NaN-boxed root slots, not raw addresses: an accessor or Proxy trap
    /// can collect and move an owner before re-entering property resolution.
    static PROTOTYPE_RESOLUTION_STACK: RefCell<crate::exception::CatchStack<u64>> = const {
        RefCell::new(crate::exception::CatchStack::new(
            crate::exception::catch_subsystem::PROTOTYPE_RESOLUTION,
        ))
    };
}
const MAX_PROTOTYPE_RESOLUTION_DEPTH: usize = 64;

/// Stack entry separating an accessor body (user code) from the resolution
/// that invoked it. Never a NaN-boxed pointer, so the root scanner skips it.
const USER_CODE_BOUNDARY: u64 = crate::value::TAG_UNDEFINED;

struct PrototypeResolutionGuard {
    depth_before: usize,
}

impl PrototypeResolutionGuard {
    fn enter(owner: usize) -> Option<Self> {
        let owner_bits = crate::value::js_nanbox_pointer(owner as i64).to_bits();
        PROTOTYPE_RESOLUTION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            // Only the owners of the CURRENT resolution count: a getter body
            // re-reading an inherited property of its own receiver is a new
            // `[[Get]]`, not a cycle (#11201).
            let segment_start = stack
                .iter()
                .rposition(|&bits| bits == USER_CODE_BOUNDARY)
                .map_or(0, |i| i + 1);
            let segment = &stack[segment_start..];
            if segment.len() >= MAX_PROTOTYPE_RESOLUTION_DEPTH || segment.contains(&owner_bits) {
                return None;
            }
            let depth_before = stack.len();
            crate::gc::runtime_write_barrier_root_nanbox(owner_bits);
            stack.push(owner_bits);
            Some(Self { depth_before })
        })
    }
}

impl Drop for PrototypeResolutionGuard {
    fn drop(&mut self) {
        PROTOTYPE_RESOLUTION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            // js_throw restores this stack before choosing the direct or
            // system-unwinder transport. Cleanup after that restore must not
            // pop a still-live outer resolution entry.
            if stack.len() > self.depth_before {
                stack.truncate(self.depth_before);
            }
        });
    }
}

/// Held across an accessor body invoked during inherited-property resolution.
/// Without it the body's own reads saw the outer resolution's owners and
/// treated them as a prototype cycle: `Object.create(new C())` with
/// `get g() { return this.x }` read `x` as `undefined` (#11201). Free when no
/// resolution is in progress. An exception that unwinds past it is covered by
/// the resolution-stack savepoint, which truncates the boundary away.
pub(crate) struct UserCodeResolutionBoundary {
    depth_before: Option<usize>,
}

impl UserCodeResolutionBoundary {
    #[inline]
    pub(crate) fn enter() -> Self {
        let depth_before = PROTOTYPE_RESOLUTION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let depth = stack.len();
            (depth != 0).then(|| {
                stack.push(USER_CODE_BOUNDARY);
                depth
            })
        });
        Self { depth_before }
    }
}

impl Drop for UserCodeResolutionBoundary {
    fn drop(&mut self) {
        if let Some(depth) = self.depth_before {
            PROTOTYPE_RESOLUTION_STACK.with(|stack| {
                let mut stack = stack.borrow_mut();
                if stack.len() > depth {
                    stack.truncate(depth);
                }
            });
        }
    }
}

#[inline]
pub(crate) fn resolution_stack_savepoint() -> usize {
    PROTOTYPE_RESOLUTION_STACK.with(|stack| stack.borrow().len())
}

pub(crate) fn resolution_stack_restore(depth: usize) {
    PROTOTYPE_RESOLUTION_STACK.with(|stack| stack.borrow_mut().truncate(depth));
}

/// GC scanner for owners held across recursive inherited-property resolution.
///
/// Getters and Proxy traps may collect before re-entering this resolver. The
/// scanner both keeps each active owner alive and rewrites its stack slot after
/// evacuation so the repeated-owner check remains an identity check.
pub(crate) fn scan_prototype_resolution_stack_roots_mut(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
) {
    PROTOTYPE_RESOLUTION_STACK.with(|stack| {
        for owner_bits in stack.borrow_mut().iter_mut() {
            visitor.visit_nanbox_u64_slot(owner_bits);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_resolution_stack_enter_and_forget(owner: usize) -> bool {
    let Some(guard) = PrototypeResolutionGuard::enter(owner) else {
        return false;
    };
    std::mem::forget(guard);
    true
}

/// Latched true by the first recorded `Object.setPrototypeOf`. Lets hot
/// per-object probes (e.g. JSON.stringify's `toJSON` fast-negative check,
/// #6009) skip the map mutex entirely in processes that never re-prototype
/// an object — the overwhelmingly common case.
static OBJECT_PROTOTYPES_NONEMPTY: AtomicBool = AtomicBool::new(false);

/// Latched true by the first `OBJECT_META_FLAG_USER_PROTO_OVERRIDE` a receiver
/// is ever given — i.e. the first `Object.setPrototypeOf` / `util.inherits`
/// that re-points a live object's `[[Prototype]]` away from its class default.
///
/// The flag lives on the receiver's meta record, so asking "does this object
/// have one?" costs two dependent loads — but only after the caller has
/// already found the object. `instanceof`'s `util.inherits` escape hatch has
/// to look up TWO class declaration prototypes through the class registry
/// before it can ask, and that pair of registry probes was the single largest
/// cost of a `o instanceof C` MISS (~130 instructions each, on a path whose
/// whole budget was 669). This latch answers for the entire process in one
/// relaxed-acquire load.
///
/// Conservative by construction: it is set, never cleared, and it is stored
/// BEFORE the flag it guards (same discipline as [`OBJECT_PROTOTYPES_NONEMPTY`]
/// above), so any reader that could observe the flag already observes the
/// latch. A false positive costs a probe pair; a false negative is impossible.
static USER_PROTO_OVERRIDE_EVER: AtomicBool = AtomicBool::new(false);

/// Has any object in this process ever been given a user `[[Prototype]]`
/// override? A `false` proves `object_has_user_prototype_override` would
/// answer `false` for every receiver, so a caller may skip whatever work it
/// would need to do to ask.
#[inline]
pub(crate) fn any_user_prototype_override() -> bool {
    USER_PROTO_OVERRIDE_EVER.load(Ordering::Acquire)
}

/// #10362: mark `obj_ptr`'s own header as an owner in the residual registry.
///
/// Called under the registry lock and BEFORE the insert, the same discipline
/// `OBJECT_PROTOTYPES_NONEMPTY` uses one line below: the proof is published
/// before the fact it guards, so a reader that can observe the entry already
/// observes the bit.
///
/// Silently does nothing for a `GC_TYPE_OBJECT` owner. Such an owner normally
/// never reaches the registry at all (`meta_capable_object` takes it), but it
/// can when that function turns it away for a non-type reason — and bit 6 means
/// `OBJ_FLAG_NULL_PROTO` there, so it must not be reused. Those owners keep the
/// latch-only gate, which is what every owner had before this change.
unsafe fn set_residual_proto_owner_bit(obj_ptr: usize) {
    #[cfg(test)]
    if residual_proto_bit_sabotage::suppressed() {
        return;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj_ptr) else {
        return;
    };
    if header.obj_type == crate::gc::GC_TYPE_OBJECT {
        return;
    }
    let header = (obj_ptr as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
    (*header)._reserved |= crate::gc::GC_RESIDUAL_PROTO_OWNER;
}

/// Can the cell at `header` own a residual-prototype entry, judged from its own
/// header rather than from the process-global latch?
///
/// This is the per-owner half of the registry's gate. Callers keep asking
/// [`object_static_prototypes_maybe_nonempty`] FIRST — it is one byte load and
/// false for any process that never re-prototyped a non-object — and ask this
/// second, which is what stops an ARMED process paying per traced cell.
///
/// Conservative for `GC_TYPE_OBJECT`: see `set_residual_proto_owner_bit`.
///
/// # Safety
///
/// `header` is a readable `GcHeader` of a live allocation.
#[inline]
pub(crate) unsafe fn residual_entry_possible_for(header: *const crate::gc::GcHeader) -> bool {
    if (*header).obj_type == crate::gc::GC_TYPE_OBJECT {
        return true;
    }
    (*header)._reserved & crate::gc::GC_RESIDUAL_PROTO_OWNER != 0
}

/// The invariant the collector's gates rest on: a LIVE non-object owner that
/// has an entry in the registry carries the bit.
///
/// Asserted in test and debug builds — including `cargo test --release`, how the
/// GC suites run — for the same reason
/// `gc::layout::transfer::assert_relocation_copied_the_header` is: a test proves
/// today's code, an assertion proves tomorrow's. A future path that inserts an
/// entry without the bit would make every collector gate skip that owner's
/// prototype edge, which is #10493's bug exactly — correct before a collection,
/// wrong after, exit code 0 and no warning.
///
/// Only this direction is an invariant. The reverse (bit set implies an entry)
/// is deliberately NOT asserted: the bit is set-only, and the two rekey paths
/// remove-then-insert with the lock released in between, so a bit without an
/// entry is a legal transient and a benign steady state.
#[inline]
pub(crate) unsafe fn debug_assert_residual_owner_bit(obj_ptr: usize) {
    #[cfg(any(test, debug_assertions))]
    {
        #[cfg(test)]
        if residual_proto_bit_sabotage::suppressed() {
            return;
        }
        if let Some(header) = crate::value::addr_class::try_read_gc_header(obj_ptr) {
            if header.obj_type != crate::gc::GC_TYPE_OBJECT {
                assert!(
                    header._reserved & crate::gc::GC_RESIDUAL_PROTO_OWNER != 0,
                    "residual prototype registry: owner {obj_ptr:#x} (obj_type {}) has an \
                     entry but not `GC_RESIDUAL_PROTO_OWNER`, so every collector gate will \
                     skip its prototype edge — the prototype is neither retained nor \
                     rewritten (#10493's failure mode)",
                    header.obj_type
                );
            }
        }
    }
    #[cfg(not(any(test, debug_assertions)))]
    {
        let _ = obj_ptr;
    }
}

/// Test-only sabotage for [`set_residual_proto_owner_bit`]: the bit is never
/// set, so every per-owner gate falls back to "no entry here" and the collector
/// skips the prototype edge.
///
/// Both tests in `gc/tests/residual_prototype_relocation.rs` MUST fail while
/// this is armed. If they pass, the bit is not load-bearing and is
/// documentation — the failure mode CLAUDE.md calls "a gate that cannot fail".
#[cfg(test)]
pub(crate) mod residual_proto_bit_sabotage {
    use std::cell::Cell;

    thread_local! {
        static SUPPRESSED: Cell<bool> = const { Cell::new(false) };
    }

    pub(crate) fn suppressed() -> bool {
        SUPPRESSED.with(Cell::get)
    }

    pub(crate) struct Guard(bool);

    impl Guard {
        pub(crate) fn arm() -> Self {
            Self(SUPPRESSED.with(|s| s.replace(true)))
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            SUPPRESSED.with(|s| s.set(self.0));
        }
    }
}

fn get_object_prototypes() -> &'static Mutex<HashMap<usize, u64>> {
    crate::once_init::get_or_init(&OBJECT_PROTOTYPES, || Mutex::new(HashMap::new()))
}

/// #6759 Phase B: classify `obj_ptr` as a genuine shaped `GC_TYPE_OBJECT`
/// whose header can carry the per-object meta record. Everything else —
/// arrays, typed arrays, native handle-band ids, proxy ids, and the dedicated
/// `GC_TYPE_REGEXP` cell — returns `None` and stays on the residual registry.
/// The classification is a pure function of the allocation, so an owner is
/// always on exactly one of the two storages.
pub(crate) unsafe fn meta_capable_object(obj_ptr: usize) -> Option<*mut crate::ObjectHeader> {
    if !crate::value::addr_class::is_above_handle_band(obj_ptr)
        // ArrayBuffer / SharedArrayBuffer / DataView use BufferHeader storage.
        // Some of those headers pass the legacy ObjectHeader validity probe,
        // but they do not have an ObjectMeta slot at the ObjectHeader offset.
        || crate::buffer::is_registered_buffer(obj_ptr)
        || !crate::object::is_valid_obj_ptr(obj_ptr as *const u8)
    {
        return None;
    }
    let header = crate::value::addr_class::try_read_gc_header(obj_ptr)?;
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    Some(obj_ptr as *mut crate::ObjectHeader)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PrototypeLinkKind {
    ClassDefault,
    ClassEvaluation,
    RuntimeWiring,
    UserOverride,
    FreshObject,
}

/// Record runtime prototype wiring while preserving the loud setter's cache
/// invalidations and shape-semantic transition. This deliberately does not
/// claim that the user replaced the receiver's prototype.
pub fn object_set_static_prototype(obj_ptr: usize, proto_bits: u64) {
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::RuntimeWiring)
}

/// Record a prototype selected by a user-facing operation such as
/// `Object.setPrototypeOf` or an object-literal `__proto__`. This has the same
/// invalidation behavior as the loud runtime setter and additionally publishes
/// the dedicated user-origin signal consumed by class dispatch.
pub(crate) fn object_set_user_prototype(obj_ptr: usize, proto_bits: u64) {
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::UserOverride)
}

/// Construct-path variant: link a fresh instance to its class-DEFAULT
/// prototype (the synthetic-class `F.prototype` object). Unlike a user
/// `setPrototypeOf`, this chain is identical for every instance of the class,
/// so it neither flushes class-keyed store plans (`object::prop_plan`) nor
/// marks the instance as chain-divergent — later mutations that could change
/// the verdict (`F.prototype = other`, descriptor installs on the proto)
/// bump the epoch at their own entry points. Calling the loud variant here
/// flushed the plan cache on EVERY function-ctor construction, which kept it
/// permanently cold in fiber-heavy workloads.
pub(crate) fn object_link_class_default_prototype(obj_ptr: usize, proto_bits: u64) {
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::ClassDefault)
}

/// An evaluated class and its instances share a prototype within that
/// evaluation, but not with other evaluations of the same template (#9502).
/// Preserve that distinction for property/method lookup and class-keyed caches.
pub(crate) fn object_link_class_evaluation_prototype(obj_ptr: usize, proto_bits: u64) {
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::ClassEvaluation)
}

/// Install Object.create's individual chain before the object escapes. It
/// needs the user-override dispatch guards, but cannot invalidate any existing
/// receiver's store plans or element-shape proofs.
pub(crate) fn object_link_created_prototype(obj_ptr: usize, proto_bits: u64) {
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::FreshObject)
}

fn object_set_static_prototype_impl(obj_ptr: usize, proto_bits: u64, link_kind: PrototypeLinkKind) {
    let prototype_diverged = link_kind != PrototypeLinkKind::ClassDefault;
    let user_override = matches!(
        link_kind,
        PrototypeLinkKind::UserOverride | PrototypeLinkKind::FreshObject
    );
    if obj_ptr == 0 {
        return;
    }
    // Whatever else this link does, the TARGET is now somebody's prototype, so
    // a later structural mutation of it is invisible to everything below it.
    // The inherited-read cache refuses to record an unmarked hop, so a link
    // kind missing from this funnel costs a cache hit and can never leave a
    // stale entry (`object::proto_validity`). Marking allocates a meta record,
    // so it happens BEFORE this function takes any raw pointer of its own.
    //
    // #10868 lever (iv): the mark returns the prototype's stable serial, read
    // from the meta pointer AFTER the mark's allocation, and it is carried as a
    // plain u64 to the divergence below rather than re-read through a pointer
    // that allocation may have moved.
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner_handle = scope.root_raw_mut_ptr(obj_ptr as *mut u8);
    let prototype_handle = scope.root_heap_word_u64(proto_bits);
    let (prototype_serial, obj_ptr): (Option<u64>, *mut u8) =
        owner_handle.across_mut::<u8, _>(|| unsafe {
            let prototype = crate::value::JSValue::from_bits(proto_bits);
            if prototype.is_pointer() {
                crate::object::proto_validity::mark_object_as_prototype(
                    prototype.as_pointer::<crate::ObjectHeader>() as usize,
                )
            } else if proto_bits == crate::value::TAG_NULL {
                Some(crate::object::proto_validity::NULL_PROTOTYPE_SERIAL)
            } else {
                None
            }
        });
    let obj_ptr = obj_ptr as usize;
    let proto_bits = prototype_handle.get_heap_word_u64();
    if !ARRAY_TARGET_PROTO_RECORDED.load(Ordering::Relaxed)
        && obj_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000
        && crate::value::addr_class::is_above_handle_band(obj_ptr)
        && crate::object::is_valid_obj_ptr(obj_ptr as *const u8)
    {
        let obj_type = unsafe {
            let hdr =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_ARRAY || obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            ARRAY_TARGET_PROTO_RECORDED.store(true, Ordering::Relaxed);
            crate::array::invalidate_array_index_fast_path();
        }
    }
    // A per-instance prototype override invalidates class-keyed interception
    // verdicts (the overridden chain can differ from the class chain), and the
    // object itself must never satisfy a class-keyed plan again.
    if matches!(
        link_kind,
        PrototypeLinkKind::RuntimeWiring | PrototypeLinkKind::UserOverride
    ) {
        crate::object::prop_plan::prop_plan_epoch_bump();
        // #7480: a `[[Prototype]]` swap on a live instance is prototype
        // surgery — the same class of event as writing onto `C.prototype`, so
        // it retires every outstanding element-shape proof. Deliberately
        // restricted to changes of existing chains: fresh default/evaluation
        // links cannot invalidate a proof about a previously allocated object.
        crate::array::invalidate_all_element_shapes();
    }
    // #6759 Phase B: shaped objects store the recorded prototype in their
    // own meta record; only non-object owners fall through to the residual
    // registry.
    unsafe {
        if let Some(obj) = meta_capable_object(obj_ptr) {
            // `object_meta_ensure` allocates and may evacuate the owner. Keep
            // both the caller's pointer and the prototype rooted, then reload
            // them before the stores below.
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);
            let proto_handle = scope.root_heap_word_u64(proto_bits);
            let (meta, obj) = obj_handle.across_mut::<crate::object::ObjectHeader, _>(|| {
                crate::object::object_meta_ensure(obj)
            });
            let proto_bits = proto_handle.get_heap_word_u64();
            (*meta).prototype = proto_bits;
            if prototype_diverged {
                (*meta).flags |= crate::object::OBJECT_META_FLAG_PROTO_DIVERGED;
            }
            if user_override {
                // Latch BEFORE the flag: a reader that observes the flag must
                // already observe the latch (see `USER_PROTO_OVERRIDE_EVER`).
                USER_PROTO_OVERRIDE_EVER.store(true, Ordering::Release);
                (*meta).flags |= crate::object::OBJECT_META_FLAG_USER_PROTO_OVERRIDE;
            }
            if link_kind == PrototypeLinkKind::ClassEvaluation {
                (*meta).flags |= crate::object::OBJECT_META_FLAG_CLASS_EVALUATION_PROTO;
            }
            // GC_STORE_AUDIT(BARRIERED): meta-record prototype slot store —
            // the record is an arena allocation, so the ordinary object-slot
            // barrier applies (parent = the meta record).
            crate::gc::runtime_write_barrier_slot(
                meta as usize,
                &(*meta).prototype as *const u64 as usize,
                proto_bits,
            );
            #[cfg(feature = "shape-mint-diag")]
            if prototype_diverged {
                crate::object::shape_mint_census::note_proto_divergence(
                    crate::object::shapes::object_shape_stamp(obj),
                    proto_bits,
                );
            }
            // The [[Prototype]] is a SHAPE fact, for every link kind: the
            // receiver moves to the shape naming its new prototype. A class-
            // default link is not exempt — `F.prototype = other` followed by
            // `new F()` otherwise leaves old and new instances on one shape
            // over two chains. Same predecessor + same prototype reaches the
            // same shape, so construction shares shapes as before. A
            // prototype with no serial (a function, array or typed array)
            // gets an identity of its own.
            let proto_id = match prototype_serial {
                Some(_) => crate::object::shapes::object_proto_id(obj),
                None => crate::object::shapes::fresh_unique_proto_id(),
            };
            crate::object::shapes::transition_object_shape_prototype(obj, proto_id);
            return;
        }
    }
    let mut slot_addr = 0usize;
    if let Ok(mut map) = get_object_prototypes().lock() {
        // Latch BEFORE the insert, and UNDER THE LOCK (#7737).
        //
        // Before the insert, because a concurrent `object_static_prototype`
        // that observed the latch in the insert-but-before-the-store window
        // would skip the mutex and miss an already-recorded prototype.
        //
        // Under the lock, because the latch is now CLEARED when a prune
        // empties the map. With the store outside, this interleaving loses an
        // entry: writer stores `true`; pruner takes the lock, retains to
        // empty, clears the latch; writer then takes the lock and inserts —
        // leaving a non-empty map with the latch false, which every reader
        // skips. Serialising both under the same mutex makes that impossible.
        // The publish property is unchanged: a reader that sees `true` takes
        // the lock and therefore sees whatever the writer committed.
        OBJECT_PROTOTYPES_NONEMPTY.store(true, Ordering::Release);
        // #10362: the per-OWNER half of the same proof, published under the
        // same lock and before the same insert, for the same reason.
        unsafe { set_residual_proto_owner_bit(obj_ptr) };
        let slot = map.entry(obj_ptr).or_insert(0);
        *slot = proto_bits;
        slot_addr = slot as *mut u64 as usize;
    }
    if slot_addr != 0 {
        crate::gc::runtime_write_barrier_external_slot(obj_ptr, slot_addr, proto_bits);
    }
}

/// Look up the recorded prototype bits for an object, if any. Returns `None`
/// when no explicit prototype has been recorded (the object still has its
/// default prototype); `Some(TAG_NULL)` when it was explicitly set to `null`.
pub fn object_static_prototype(obj_ptr: usize) -> Option<u64> {
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_decoded_pointer(obj_ptr);
    }
    // #6759 Phase B: a shaped object answers from its own meta record — two
    // dependent loads, no global latch, no mutex — and NEVER has a residual
    // registry entry (the write path classifies identically), so a meta
    // miss for a shaped object is authoritative.
    unsafe {
        if let Some(obj) = meta_capable_object(obj_ptr) {
            let meta = (*obj).meta;
            if !meta.is_null() {
                let bits = (*meta).prototype;
                if bits != 0 {
                    return Some(bits);
                }
            }
            return None;
        }
    }
    if !OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire) {
        return None;
    }
    let recorded = get_object_prototypes()
        .lock()
        .ok()
        .and_then(|map| map.get(&obj_ptr).copied());
    // #10362: the invariant every collector gate rests on, checked on the read
    // paths that are NOT gated by the bit — asserting it inside a bit-gated
    // path would be vacuous.
    if recorded.is_some() {
        unsafe { debug_assert_residual_owner_bit(obj_ptr) };
    }
    recorded
}

/// Look up the residual prototype registry for a caller that has already
/// proved its receiver is not a shaped `GC_TYPE_OBJECT`.
///
/// `RegExp` cells meet that precondition. Keeping it explicit lets their hot
/// view-mode proof read the empty latch FIRST and return without paying
/// `meta_capable_object`'s buffer/object/header classification. The ordinary
/// [`object_static_prototype`] remains the entry for unclassified receivers
/// and still checks object-owned metadata before consulting this registry.
#[inline]
// #9917 added this for the recorded canonical-test-site proof, whose only
// caller is regex-engine gated; without the feature it is dead in a product
// build. Same gate as the rest of that surface (#9970).
#[cfg(any(test, feature = "regex-engine"))]
pub(crate) fn object_static_prototype_known_non_meta(obj_ptr: usize) -> Option<u64> {
    if !OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire) {
        return None;
    }
    let recorded = get_object_prototypes()
        .lock()
        .ok()
        .and_then(|map| map.get(&obj_ptr).copied());
    if recorded.is_some() {
        unsafe { debug_assert_residual_owner_bit(obj_ptr) };
    }
    recorded
}

#[inline]
fn object_has_prototype_flag(obj_ptr: usize, flag: u64) -> bool {
    unsafe {
        let Some(obj) = meta_capable_object(obj_ptr) else {
            return false;
        };
        let meta = (*obj).meta;
        !meta.is_null() && (*meta).flags & flag != 0
    }
}

/// True when this receiver's recorded prototype diverges from its class
/// default, regardless of whether runtime wiring or a user-facing operation
/// selected it. Cache guards use this conservative signal.
#[inline]
/// Does this receiver's `[[Prototype]]` chain END IN AN EXPLICIT `null`?
///
/// Perry bakes class ids at allocation time, so every "the own-key scan
/// missed, what does this object inherit?" path in the runtime falls back to
/// the receiver's CLASS surface — its vtable, its declaration prototype, or
/// `Object.prototype`. That fallback is right for an object whose chain was
/// never touched, and it is WRONG for one whose chain was explicitly ended:
/// `Object.setPrototypeOf(o, null)` says there is nothing above `o` any more,
/// and the fallback walked up anyway and answered from the prototype `o` was
/// BORN with (#10827).
///
/// A chain ends explicitly when a hop carries a recorded `TAG_NULL`
/// (`Object.setPrototypeOf(x, null)`, `__proto__ = null`) or is a cell born
/// with no prototype at all (`Object.create(null)`, `OBJ_FLAG_NULL_PROTO`). It
/// does NOT end explicitly when a hop simply has no record: that is an
/// ordinary object standing on the class default, which is exactly the case
/// the fallback exists for.
///
/// Cost is paid only on a MISS, and only walks hops that carry a record: an
/// ordinary receiver answers `false` from one absent meta record plus one
/// header bit.
pub(crate) fn prototype_chain_ends_in_explicit_null(obj_ptr: usize) -> bool {
    let mut current = obj_ptr;
    // The same bound the generic chain walk uses. A cycle cannot be built
    // through `setPrototypeOf` (it refuses one), but a bound is cheaper than
    // trusting that from here.
    for _ in 0..32 {
        if unsafe { cell_is_born_null_proto(current) } {
            return true;
        }
        match object_static_prototype(current) {
            // No per-instance record on this hop. The chain does not stop
            // here: it continues through the hop's CLASS, which is where a
            // `class K {}` instance keeps `K.prototype`. Following it is what
            // makes `Object.setPrototypeOf(K.prototype, null)` visible to an
            // instance that was never itself re-prototyped — the case where
            // the receiver's own guard and the holder's both see nothing.
            None => {
                let next = unsafe { class_link_prototype(current) };
                if next == 0 || next == current || next == obj_ptr {
                    return false;
                }
                current = next;
            }
            Some(TAG_NULL) => return true,
            Some(bits) => {
                let top16 = bits >> 48;
                let next = if top16 == 0x7FFD {
                    (bits & 0x0000_FFFF_FFFF_FFFF) as usize
                } else if top16 == 0
                    && crate::value::addr_class::is_above_handle_band(bits as usize)
                {
                    // The canonical band predicate rather than a hand-typed
                    // `> 0x10000` floor: `addr_class` owns where the handle
                    // band ends, and a literal here is the shape #6321 fixed.
                    bits as usize
                } else {
                    return false;
                };
                if next == 0 || next == current || next == obj_ptr {
                    return false;
                }
                current = next;
            }
        }
    }
    false
}

/// The prototype a cell reaches through its CLASS rather than through a
/// per-instance record: the declared `class X {}` prototype when there is one,
/// otherwise the synthetic-class prototype object. 0 when the cell has no
/// class link — an ordinary `{}` (class id 0) or a kind whose chain is not
/// resolved this way.
///
/// Deliberately the same precedence `native_get::try_data_get_bytes` uses to
/// resolve the next hop, so this predicate walks the chain a READ walks and
/// cannot answer about a hop the read never visits.
#[inline]
unsafe fn class_link_prototype(obj_ptr: usize) -> usize {
    if obj_ptr == 0 || !crate::object::is_valid_obj_ptr(obj_ptr as *const u8) {
        return 0;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj_ptr) else {
        return 0;
    };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return 0;
    }
    let class_id = (*(obj_ptr as *const crate::ObjectHeader)).class_id;
    if class_id == 0 {
        return 0;
    }
    let declared = super::class_decl_prototype_object(class_id);
    if !declared.is_null() {
        return declared as usize;
    }
    super::class_prototype_object(class_id) as usize
}

/// Was this cell allocated with no prototype (`Object.create(null)`,
/// `querystring.parse`)? That is `OBJ_FLAG_NULL_PROTO`, the header bit #1175
/// added, and it is the born-null half of the question
/// [`prototype_chain_ends_in_explicit_null`] asks.
#[inline]
unsafe fn cell_is_born_null_proto(obj_ptr: usize) -> bool {
    if obj_ptr == 0 || !crate::object::is_valid_obj_ptr(obj_ptr as *const u8) {
        return false;
    }
    let Some(header) = crate::value::addr_class::try_read_gc_header(obj_ptr) else {
        return false;
    };
    header.obj_type == crate::gc::GC_TYPE_OBJECT
        && header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
}

pub(crate) fn object_has_prototype_divergence(obj_ptr: usize) -> bool {
    object_has_prototype_flag(obj_ptr, crate::object::OBJECT_META_FLAG_PROTO_DIVERGED)
}

/// True only when a user-facing operation selected this receiver's prototype.
/// Runtime wiring can use the same metadata record and loud invalidations, but
/// it deliberately leaves this distinct bit clear.
#[inline]
pub(crate) fn object_has_user_prototype_override(obj_ptr: usize) -> bool {
    object_has_prototype_flag(obj_ptr, crate::object::OBJECT_META_FLAG_USER_PROTO_OVERRIDE)
}

/// Whether ordinary property lookup must consult the receiver's own chain
/// before the shared class vtable: user overrides and evaluated classes both
/// have this requirement; unrelated runtime prototype wiring does not.
#[inline]
pub(crate) fn object_has_individual_class_prototype(obj_ptr: usize) -> bool {
    object_has_prototype_flag(
        obj_ptr,
        crate::object::OBJECT_META_FLAG_USER_PROTO_OVERRIDE
            | crate::object::OBJECT_META_FLAG_CLASS_EVALUATION_PROTO,
    )
}

pub(crate) fn default_object_prototype_bits() -> Option<u64> {
    let object_ctor = super::js_get_global_this_builtin_value(b"Object".as_ptr(), 6);
    let ctor_bits = object_ctor.to_bits();
    if (ctor_bits >> 48) != 0x7FFD {
        return None;
    }
    let ctor_ptr = (ctor_bits & crate::value::POINTER_MASK) as usize;
    if ctor_ptr == 0 {
        return None;
    }
    let proto = crate::closure::closure_get_dynamic_prop(ctor_ptr, "prototype");
    let proto_bits = proto.to_bits();
    if (proto_bits >> 48) == 0x7FFD {
        Some(proto_bits)
    } else {
        None
    }
}

pub(crate) unsafe fn default_object_prototype_for_owner(obj_ptr: usize) -> Option<u64> {
    if obj_ptr == 0 {
        return None;
    }
    let obj = obj_ptr as *const crate::ObjectHeader;
    if !super::is_valid_obj_ptr(obj as *const u8) {
        return None;
    }
    let gc = super::gc_header_for(obj);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
        return None;
    }
    if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT
        || ((*obj).class_id != 0 && !super::is_anon_shape_class_id((*obj).class_id))
    {
        return None;
    }
    let proto_bits = default_object_prototype_bits()?;
    let proto_ptr = (proto_bits & crate::value::POINTER_MASK) as usize;
    if proto_ptr == 0 || proto_ptr == obj_ptr {
        return None;
    }
    Some(proto_bits)
}

/// Death pruning (2026-07-09 GC audit wave 2): entries survived owner death,
/// so the recorded prototype object stayed strongly rooted forever and a
/// fresh object at a recycled address inherited the dead owner's prototype
/// (dangling/wrong `getPrototypeOf`, phantom inherited reads).
/// `is_dead_owner` is one of the GC's deadness predicates (`gc::dead_owner`).
pub(crate) fn prune_dead_object_prototype_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    if !OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire) {
        return;
    }
    if let Ok(mut map) = get_object_prototypes().lock() {
        map.retain(|owner, _| !is_dead_owner(*owner));
        // #7737: release the latch when the registry drains.
        //
        // It used to be one-way. Since #7733 the evacuation move hook reads it
        // once per moved object, so a SINGLE `Object.setPrototypeOf` against a
        // non-meta-capable owner — anywhere in a process's lifetime, even one
        // that later dies and is pruned right here — permanently disabled that
        // fast path for the rest of the run. That is #7510's "one immortal
        // side-table entry nullified every is_empty() fast path" recurring.
        //
        // Safe to clear here because the set is now under this same lock: no
        // insert can be in flight past its latch store while we hold it.
        if map.is_empty() {
            OBJECT_PROTOTYPES_NONEMPTY.store(false, Ordering::Release);
        }
    }
}

/// Can the residual owner registry hold an entry at all?
///
/// The latch is stored (`Release`) before the first insert, so `false` proves
/// the registry empty — the same proof [`object_static_prototype_owner_moved`]
/// makes on entry, exposed so the relocation funnel
/// (`gc/layout/transfer.rs`) can decide without the call (#10362).
#[inline]
pub(crate) fn object_static_prototypes_maybe_nonempty() -> bool {
    OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire)
}

/// Can a cell of `obj_type` own an entry in the residual registry?
///
/// The registry's population is every owner [`meta_capable_object`] turns
/// away, and the recorder is reached with whatever the caller holds:
/// `Object.setPrototypeOf` with an array, lazy JSON array, Map, Set, Error,
/// Promise, Date, RegExp or Temporal cell, `dyn_eval` with a closure. Only the
/// kinds that can never be a receiver stand outside it: strings and bigints
/// are primitives, meta records and compiled regex programs are internal.
/// `GC_TYPE_OBJECT` stays inside — its prototypes live in its meta record, and
/// the registry's obligations were always met for it too.
///
/// The collector keys both of the registry's per-owner obligations on this
/// one predicate: the relocation rekey (`gc/layout/transfer.rs`) and the
/// value visit (`gc/layout_slot_visit.rs`). Both used to be wired to arrays
/// and ordinary objects by hand, so every other movable owner lost its
/// explicit prototype at its first relocation, and none had the prototype
/// value traced or rewritten.
#[inline]
pub(crate) fn residual_prototype_owner_type(obj_type: u8) -> bool {
    !matches!(
        obj_type,
        crate::gc::GC_TYPE_STRING
            | crate::gc::GC_TYPE_BIGINT
            | crate::gc::GC_TYPE_OBJECT_META
            | crate::gc::GC_TYPE_REGEX_PROGRAM
    )
}

/// Migrate the residual side-table entry when an owner's allocation address
/// changes, either through moving GC or an `ArrayHeader` growth replacement.
/// Mirrors `closure_dynamic_props_owner_moved`.
pub(crate) fn object_static_prototype_owner_moved(old_owner: usize, new_owner: usize) {
    if old_owner == 0 || new_owner == 0 || old_owner == new_owner {
        return;
    }
    // The residual registry is EMPTY until a non-meta-capable owner records a
    // prototype, and the latch is stored (`Release`) *before* that insert — so
    // a `false` read here proves there is no entry to migrate. Its two sibling
    // readers (`object_static_prototype`, `prune_dead_object_prototype_owners`)
    // already gate on it; this one did not, so every evacuated object took a
    // process-global `Mutex<HashMap>` and paid a SipHash probe against an empty
    // map. On a promotion-heavy workload that is one lock + one hash per moved
    // object (2.5 M of each on `gc-handoff/bench/retain.ts`), and it showed up
    // as `pthread_mutex_lock` + `RandomState` in a single-threaded profile.
    if !OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire) {
        return;
    }
    if let Ok(mut map) = get_object_prototypes().lock() {
        if let Some(proto_bits) = map.remove(&old_owner) {
            map.insert(new_owner, proto_bits);
        }
    }
}

/// GC scanner: visit the stored prototype-value slot for `owner` so a moving
/// collector can rewrite a forwarded prototype pointer. A `TAG_NULL` entry is
/// not a pointer, so the collector simply leaves it unchanged.
pub(crate) fn visit_object_static_prototype_slot_mut(
    owner: usize,
    mut visit: impl FnMut(*mut u64),
) {
    if owner == 0 {
        return;
    }
    // The residual registry is EMPTY until a non-meta-capable owner records a
    // prototype, and the latch is stored (`Release`) *before* that insert, so
    // a `false` read proves there is nothing here to visit. Its siblings
    // (`object_static_prototype`, `object_static_prototype_owner_moved`,
    // `prune_dead_object_prototype_owners`) already gate on it; THIS one is
    // the collector's per-object rewrite hook, so without the gate every
    // traced object took a process-global `Mutex<HashMap>` and paid a SipHash
    // probe against an empty map. Measured on `gc-handoff/bench/retain.ts`:
    // `pthread_mutex_lock` and `RandomState::hash_one` were both visible under
    // the mark drain in a single-threaded profile.
    if !OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire) {
        return;
    }
    // Take the entry OUT and run the visit with the lock RELEASED: a
    // copying-minor rewrite visitor can move the prototype object, and
    // move fixup re-enters `object_static_prototype_owner_moved`, which
    // takes this same lock — visiting under it self-deadlocks the
    // collector. Same hazard and fix as the closure static-prototype
    // visitor in `closure::dynamic_props`.
    let Some(mut proto_bits) = get_object_prototypes()
        .lock()
        .ok()
        .and_then(|mut map| map.remove(&owner))
    else {
        return;
    };
    visit(&mut proto_bits as *mut u64);
    // The visit can forward the owner itself (self-referential
    // prototype); re-key the entry to the forwarded address.
    let new_owner = unsafe {
        crate::value::addr_class::try_read_gc_header(owner)
            .filter(|h| h.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0)
            .map(|h| crate::gc::forwarding_address(h as *const _) as usize)
            .unwrap_or(owner)
    };
    if let Ok(mut map) = get_object_prototypes().lock() {
        map.insert(new_owner, proto_bits);
    }
}

/// Resolve an inherited property read for an object whose own keys did not
/// contain `key`. Walks the recorded prototype chain (bounded to guard against
/// user-induced cycles). Returns `Some(value)` when a prototype in the chain
/// has the key as an own property, else `None` (caller returns `undefined`).
///
/// `key` is the lookup key already known not to be an own property of the
/// starting object. Each hop reads via `js_object_get_field_by_name`, which is
/// the generic own+inherited getter — but because we only enter this walk after
/// an own-key miss, and the proto's own keys are what matters, re-entering the
/// generic getter on the proto naturally continues the chain.
pub(crate) fn resolve_inherited_field(
    obj_ptr: usize,
    key: *const crate::StringHeader,
) -> Option<crate::value::JSValue> {
    let proto_bits = object_static_prototype(obj_ptr)?;
    resolve_inherited_field_from_prototype(obj_ptr, proto_bits, key)
}

/// Resolve an inherited property through a known prototype while retaining
/// `obj_ptr` as the receiver for accessors and Proxy traps. Intrinsic
/// TypedArray prototypes are not recorded in `object_static_prototype`, so
/// their erased-type fallback passes the builtin prototype here directly.
pub(crate) fn resolve_inherited_field_from_prototype(
    obj_ptr: usize,
    proto_bits: u64,
    key: *const crate::StringHeader,
) -> Option<crate::value::JSValue> {
    let _guard = PrototypeResolutionGuard::enter(obj_ptr)?;
    if proto_bits == TAG_NULL {
        return None;
    }
    let top16 = proto_bits >> 48;
    let proto_ptr = if top16 == 0x7FFD {
        (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && proto_bits > 0x10000 {
        proto_bits as usize
    } else {
        return None;
    };
    if proto_ptr == 0 || proto_ptr == obj_ptr {
        return None;
    }
    // A Proxy prototype (`Object.create(proxy).x`) is a small fake pointer in
    // the proxy id band, which passes the loose `is_valid_obj_ptr` heap-range
    // check below and would then be dereferenced as an `ObjectHeader` — a
    // SIGSEGV. Route the inherited read through the proxy's `[[Get]]` (which
    // fires the get trap or forwards to the target), binding the original
    // instance as the receiver. (test262
    // Proxy/get/trap-is-{null,undefined}-target-is-proxy via
    // `Object.create(proxy)[k]`.)
    {
        let proto_val = f64::from_bits(proto_bits);
        if crate::proxy::js_proxy_is_proxy(proto_val) != 0 {
            if key.is_null() {
                return None;
            }
            let key_val = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
            let receiver = super::field_get_set::accessor_receiver_override_take()
                .unwrap_or_else(|| crate::value::js_nanbox_pointer(obj_ptr as i64));
            let v = crate::proxy::proxy_get_with_receiver(proto_val, key_val, receiver);
            return Some(crate::value::JSValue::from_bits(v.to_bits()));
        }
    }
    let proto = proto_ptr as *const crate::ObjectHeader;
    if !super::is_valid_obj_ptr(proto as *const u8) {
        return None;
    }
    // `js_object_get_field_by_name` handles its own further prototype hops
    // (recorded protos on the proto object), so this is the full walk. Bind
    // accessor getters to the original receiver while walking inherited
    // properties; otherwise prototype accessors would observe the prototype
    // object instead of the instance.
    let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits());
    let scope = crate::gc::RuntimeHandleScope::new();
    let previous_this = super::js_implicit_this_set(receiver);
    let previous_this_handle = scope.root_nanbox_f64(previous_this);
    // The recursive `get_field(proto, key)` re-derives the accessor receiver
    // from `proto`; stash the real instance so an inherited getter binds `this`
    // to it, not to the prototype.
    let prev_override = super::field_get_set::accessor_receiver_override_begin(receiver);
    let prev_override_handle = prev_override.map(|value| scope.root_nanbox_f64(value));
    let v = super::js_object_get_field_by_name(proto, key);
    super::field_get_set::accessor_receiver_override_end(
        prev_override_handle.map(|handle| handle.get_nanbox_f64()),
    );
    super::js_implicit_this_set(previous_this_handle.get_nanbox_f64());
    if v.bits() == 0x7FFC_0000_0000_0001 {
        // undefined — treat as "not present" so callers fall back cleanly.
        None
    } else {
        Some(v)
    }
}

/// Test-only: swap the process-wide "a REAL array somewhere has a custom
/// `[[Prototype]]`" latch, returning the previous value.
///
/// The latch is one-way in production and deliberately so. But a unit test
/// that legitimately retargets a real array's prototype latches it for the
/// whole binary and stands `plain_array_index_guard` down for every later
/// typed-feedback / proxy guard test in the same process — the hazard
/// `gc::tests::dead_owner_side_tables` documents inline. Once that test's
/// array is unreachable the latch's claim is no longer TRUE, which is the
/// same correction #7737 made for `OBJECT_PROTOTYPES_NONEMPTY`. Such a test
/// restores what it found; see `ArrayPrototypeLatchGuard` in
/// `dyn_eval/tests.rs`.
#[cfg(test)]
pub(crate) fn test_swap_array_static_proto_recorded(value: bool) -> bool {
    ARRAY_TARGET_PROTO_RECORDED.swap(value, Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn test_prototype_registry_latch_armed() -> bool {
    OBJECT_PROTOTYPES_NONEMPTY.load(Ordering::Acquire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_wiring_and_user_override_publish_distinct_signals() {
        let _no_move = crate::gc::GcSuppressScope::new();

        let runtime_wired = crate::object::js_object_alloc(0, 0);
        object_set_static_prototype(runtime_wired as usize, crate::value::TAG_NULL);
        let runtime_meta = unsafe { (*runtime_wired).meta };
        assert!(!runtime_meta.is_null());
        assert_ne!(
            unsafe { (*runtime_meta).flags } & crate::object::OBJECT_META_FLAG_PROTO_DIVERGED,
            0,
            "the loud runtime setter must retain its conservative divergence signal"
        );
        assert!(object_has_prototype_divergence(runtime_wired as usize));
        assert!(
            !object_has_user_prototype_override(runtime_wired as usize),
            "runtime prototype wiring must not masquerade as a user override"
        );

        let class_default = crate::object::js_object_alloc(0, 0);
        object_link_class_default_prototype(class_default as usize, crate::value::TAG_NULL);
        let class_default_meta = unsafe { (*class_default).meta };
        assert!(!class_default_meta.is_null());
        assert_eq!(
            unsafe { (*class_default_meta).flags }
                & (crate::object::OBJECT_META_FLAG_PROTO_DIVERGED
                    | crate::object::OBJECT_META_FLAG_USER_PROTO_OVERRIDE),
            0,
            "class-default links must publish neither divergence signal"
        );
        assert!(!object_has_prototype_divergence(class_default as usize));

        let evaluated = crate::object::js_object_alloc(0, 0);
        object_link_class_evaluation_prototype(evaluated as usize, crate::value::TAG_NULL);
        assert!(object_has_prototype_divergence(evaluated as usize));
        assert!(object_has_individual_class_prototype(evaluated as usize));
        assert!(!object_has_user_prototype_override(evaluated as usize));
        assert!(!object_has_individual_class_prototype(
            class_default as usize
        ));
        assert!(!object_has_individual_class_prototype(
            runtime_wired as usize
        ));

        let user_overridden = crate::object::js_object_alloc(0, 0);
        object_set_user_prototype(user_overridden as usize, crate::value::TAG_NULL);
        let user_meta = unsafe { (*user_overridden).meta };
        assert!(!user_meta.is_null());
        assert_ne!(
            unsafe { (*user_meta).flags } & crate::object::OBJECT_META_FLAG_PROTO_DIVERGED,
            0
        );
        assert!(object_has_prototype_divergence(user_overridden as usize));
        assert!(object_has_user_prototype_override(user_overridden as usize));
    }

    /// #10827. Every one of these is a case where the READ used to disagree
    /// with `in` on the same object — perry contradicting itself, which is the
    /// cleanest oracle available and the one the regression fixture asserts.
    #[test]
    fn an_explicitly_nulled_prototype_ends_the_chain() {
        let proto = crate::object::js_object_alloc(0, 4);
        let obj = crate::object::js_object_alloc(0, 4);
        object_set_static_prototype(
            obj as usize,
            crate::value::js_nanbox_pointer(proto as i64).to_bits(),
        );
        assert!(
            !prototype_chain_ends_in_explicit_null(obj as usize),
            "a chain standing on a real prototype object is not ended"
        );
        object_set_user_prototype(obj as usize, crate::value::TAG_NULL);
        assert!(
            prototype_chain_ends_in_explicit_null(obj as usize),
            "Object.setPrototypeOf(o, null) says there is nothing above o; \
             without this the class-surface fallback walked up anyway and \
             answered from the prototype o was BORN with"
        );
    }

    #[test]
    fn a_null_ended_interior_prototype_ends_the_chain_for_an_instance() {
        // The interior prototype is EXPLICITLY ended, not merely absent.
        // O -> P1 -> null. Neither O's own record nor the holder's says
        // anything: the statement is two hops up.
        let p1 = crate::object::js_object_alloc(0, 4);
        let obj = crate::object::js_object_alloc(0, 4);
        object_set_static_prototype(
            obj as usize,
            crate::value::js_nanbox_pointer(p1 as i64).to_bits(),
        );
        assert!(!prototype_chain_ends_in_explicit_null(obj as usize));
        object_set_user_prototype(p1 as usize, crate::value::TAG_NULL);
        assert!(
            prototype_chain_ends_in_explicit_null(obj as usize),
            "an interior prototype ended in null is invisible to both ends of \
             the chain"
        );
    }

    #[test]
    fn a_hop_born_without_a_prototype_ends_the_chain() {
        let born_null = crate::object::js_object_alloc_null_proto(0, 4);
        assert!(
            prototype_chain_ends_in_explicit_null(born_null as usize),
            "Object.create(null) is the born half of the same statement"
        );
        let obj = crate::object::js_object_alloc(0, 4);
        object_set_static_prototype(
            obj as usize,
            crate::value::js_nanbox_pointer(born_null as i64).to_bits(),
        );
        assert!(
            prototype_chain_ends_in_explicit_null(obj as usize),
            "pointing an object at an Object.create(null) ends ITS chain too"
        );
    }

    #[test]
    fn an_untouched_object_does_not_end_its_chain() {
        // The common case, and the one that must stay cheap and must stay
        // FALSE: an ordinary object stands on the class default, which is
        // exactly what the fallback this predicate gates exists to reach.
        let plain = crate::object::js_object_alloc(0, 4);
        assert!(!prototype_chain_ends_in_explicit_null(plain as usize));
    }

    #[test]
    fn a_recorded_prototype_cycle_does_not_hang_the_null_walk() {
        let first = crate::object::js_object_alloc(0, 0);
        let second = crate::object::js_object_alloc(0, 0);
        object_set_static_prototype(
            first as usize,
            crate::value::js_nanbox_pointer(second as i64).to_bits(),
        );
        object_set_static_prototype(
            second as usize,
            crate::value::js_nanbox_pointer(first as i64).to_bits(),
        );
        assert!(!prototype_chain_ends_in_explicit_null(first as usize));
    }

    #[test]
    fn inherited_lookup_stops_on_recorded_prototype_cycle() {
        let first = crate::object::js_object_alloc(0, 0);
        let second = crate::object::js_object_alloc(0, 0);
        object_set_static_prototype(
            first as usize,
            crate::value::js_nanbox_pointer(second as i64).to_bits(),
        );
        object_set_static_prototype(
            second as usize,
            crate::value::js_nanbox_pointer(first as i64).to_bits(),
        );
        let missing = crate::string::js_string_from_bytes(b"missing".as_ptr(), 7);
        assert!(resolve_inherited_field(first as usize, missing).is_none());
        assert!(PROTOTYPE_RESOLUTION_STACK.with(|stack| stack.borrow().is_empty()));
    }

    #[test]
    fn exception_unwind_restores_resolution_stack_savepoint() {
        let base_depth = resolution_stack_savepoint();
        let _jump_buffer = crate::exception::js_try_push();
        let first = PrototypeResolutionGuard::enter(usize::MAX - 1).unwrap();
        let second = PrototypeResolutionGuard::enter(usize::MAX).unwrap();
        assert_eq!(resolution_stack_savepoint(), base_depth + 2);

        // A real longjmp skips these drops. Forget the guards to model that
        // behavior, then replay js_throw's savepoint restoration.
        std::mem::forget(first);
        std::mem::forget(second);
        crate::exception::test_unwind_innermost_shadow_restore();
        crate::exception::js_try_end();

        assert_eq!(resolution_stack_savepoint(), base_depth);
    }

    #[test]
    fn system_unwind_drop_is_idempotent_after_resolution_restore() {
        let base_depth = resolution_stack_savepoint();
        let _jump_buffer = crate::exception::js_try_push();
        let first = PrototypeResolutionGuard::enter(usize::MAX - 1).unwrap();
        let second = PrototypeResolutionGuard::enter(usize::MAX).unwrap();
        assert_eq!(resolution_stack_savepoint(), base_depth + 2);

        crate::exception::test_unwind_innermost_shadow_restore();
        drop(second);
        drop(first);
        crate::exception::js_try_end();

        assert_eq!(resolution_stack_savepoint(), base_depth);
    }
}

#[cfg(test)]
mod latch_drain_tests_7737 {
    use super::*;

    /// #7737: the registry's "non-empty" latch must be RELEASED when a prune
    /// drains the map, not held for the life of the process.
    ///
    /// Since #7733 the evacuation move hook (`object_static_prototype_owner_moved`)
    /// reads this latch once per moved object to skip a process-global mutex
    /// and a SipHash lookup. While it was one-way, a single
    /// `Object.setPrototypeOf` against a non-meta-capable owner — anywhere in
    /// a process's lifetime, including one that dies and is pruned moments
    /// later — permanently disabled that fast path for the rest of the run.
    ///
    /// That is #7510's finding recurring: "one immortal side-table entry
    /// nullified every `is_empty()` fast path". The assertion that matters is
    /// the LAST one — that the latch comes back down — because everything
    /// before it passes with the bug present.
    #[test]
    fn a_drained_prototype_registry_releases_the_fast_path_latch() {
        let _lock = crate::gc::global_side_table_test_lock();

        // Start from a known state: drain whatever earlier tests recorded.
        prune_dead_object_prototype_owners(&|_| true);

        let owner: usize = 0x5000_0000;
        let proto_bits: u64 = 0x7FFC_0000_0000_0001;
        if let Ok(mut map) = get_object_prototypes().lock() {
            OBJECT_PROTOTYPES_NONEMPTY.store(true, Ordering::Release);
            map.insert(owner, proto_bits);
        }
        assert!(
            test_prototype_registry_latch_armed(),
            "setup: recording an owner must arm the latch"
        );

        // The owner dies and is pruned — the registry is empty again.
        prune_dead_object_prototype_owners(&|o| o == owner);
        assert!(
            get_object_prototypes()
                .lock()
                .map(|m| m.is_empty())
                .unwrap_or(false),
            "setup: the prune must actually have emptied the map"
        );

        assert!(
            !test_prototype_registry_latch_armed(),
            "#7737: the registry is empty but the latch is still armed, so \
             every evacuated object keeps paying the mutex + SipHash lookup \
             for the rest of the process"
        );
    }

    /// The collector's per-object rewrite hook now gates on the same latch.
    ///
    /// Both halves are asserted, because only the pair is a fix: a hook that
    /// skips an EMPTY registry is the optimisation, and a hook that still
    /// reaches a RECORDED entry is the thing the optimisation must not break.
    /// Without the second assertion, `return;` at the top of the function
    /// would also pass.
    #[test]
    fn the_gc_visit_hook_skips_an_empty_registry_and_still_reaches_a_recorded_one() {
        let _lock = crate::gc::global_side_table_test_lock();
        prune_dead_object_prototype_owners(&|_| true);

        // A REAL old-gen allocation, not a synthetic address: on the armed
        // path the visitor re-reads the owner's `GcHeader` to re-key a
        // self-referential prototype, so a made-up owner segfaults there. (The
        // #7737 test above never calls the visitor, which is why it can use
        // one.)
        let owner = crate::arena::arena_alloc_gc_old(64, 8, crate::gc::GC_TYPE_OBJECT) as usize;
        let proto_bits: u64 = 0x7FFC_0000_0000_0001;

        let mut visits = 0usize;
        visit_object_static_prototype_slot_mut(owner, |_| visits += 1);
        assert_eq!(
            visits, 0,
            "an empty registry must be answered by the latch, not by a \
             process-global mutex plus a SipHash probe — this hook runs once \
             per TRACED object"
        );

        if let Ok(mut map) = get_object_prototypes().lock() {
            OBJECT_PROTOTYPES_NONEMPTY.store(true, Ordering::Release);
            map.insert(owner, proto_bits);
        }
        let mut seen = 0u64;
        let mut visits = 0usize;
        visit_object_static_prototype_slot_mut(owner, |slot| {
            visits += 1;
            seen = unsafe { *slot };
        });
        assert_eq!(visits, 1, "a recorded prototype slot must still be visited");
        assert_eq!(seen, proto_bits);

        prune_dead_object_prototype_owners(&|o| o == owner);
    }
}
