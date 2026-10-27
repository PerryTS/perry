//! Observable `[[Prototype]]` for ordinary heap objects (#2820, #6759 B).
//!
//! Perry bakes class IDs at allocation time, so it cannot rewrite an object's
//! baked prototype chain. But `Object.setPrototypeOf(obj, proto)` on an
//! *ordinary* object (a `{}` literal, an `Object.create(...)` result, etc.)
//! must be observable: a later `Object.getPrototypeOf(obj)` returns the same
//! `proto`, and an inherited property read (`obj.x` where `x` lives on `proto`)
//! walks to it.
//!
//! Ordinary objects answer from their shape or ObjectMeta. Every other
//! public owner keeps the value in its ordinary property holder's ObjectMeta.
//! The bag is a traced child of its owner, so relocation and death need no
//! address registry, per-owner scanner, rekey hook or prune.
//!
//! A stored TAG_NULL distinguishes an explicit null from the unset word 0.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

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

crate::perry_thread_local! {
    /// Owners currently walking a recorded prototype chain. Although
    /// `Object.setPrototypeOf` normally rejects cycles, residual/native owners
    /// and custom-construction links can still expose a malformed chain. Keep
    /// recursive property lookup bounded and stop on a repeated owner. These
    /// are NaN-boxed root slots, not raw addresses: an accessor or Proxy trap
    /// can collect and move an owner before re-entering property resolution.
    static PROTOTYPE_RESOLUTION_STACK: RefCell<crate::exception::CatchStack<u64>> =
        const { RefCell::new(crate::exception::CatchStack::new()) };
}
const MAX_PROTOTYPE_RESOLUTION_DEPTH: usize = 64;
/// From this resolution depth on, an inherited read first skips every
/// prototype whose shape proves it lacks the key
/// (`shape_presence::first_possible_holder`),
/// so the recursion depth counts only the prototypes that may hold it or
/// that the shapes cannot decide, never the chain's length (#12698: a
/// 270-deep `Object.create` chain read `undefined` once the recursion reached
/// the cap). Shallower reads recurse directly: each recursive read's layout
/// probe already answers several hops at once.
const SKIP_PROVEN_ABSENT_FROM_DEPTH: usize = 8;

/// Bounds a prototype walk by the chain itself rather than by a hop count:
/// `repeats` reports a node the walk already visited (Brent's tortoise), so a
/// malformed cyclic chain still ends while a long acyclic chain is walked to
/// its end (#12698: a hop cap answered "absent" past it, or refused a store).
/// `Object.setPrototypeOf` rejects cycles; residual/native owners are the
/// only way to build one.
pub(crate) struct ChainCycle {
    mark: usize,
    lap: usize,
    steps: usize,
}

impl ChainCycle {
    pub(crate) const fn new() -> Self {
        Self {
            mark: 0,
            lap: 1,
            steps: 0,
        }
    }

    /// Call once per visited node, in walk order.
    #[inline]
    pub(crate) fn repeats(&mut self, node: usize) -> bool {
        if node == self.mark {
            return true;
        }
        self.steps += 1;
        if self.steps == self.lap {
            self.mark = node;
            self.lap <<= 1;
            self.steps = 0;
        }
        false
    }
}

/// Stack entry separating an accessor body (user code) from the resolution
/// that invoked it. Never a NaN-boxed pointer, so the root scanner skips it.
const USER_CODE_BOUNDARY: u64 = crate::value::TAG_UNDEFINED;

struct PrototypeResolutionGuard {
    depth_before: usize,
    /// Resolutions of the current `[[Get]]` already on the stack below this one.
    segment_depth: usize,
}

impl PrototypeResolutionGuard {
    /// Enter one resolution step owned by the calling frame
    /// (`#[inline(always)]`: the frame mark must be read in the owner).
    #[inline(always)]
    fn enter(owner: usize) -> Option<Self> {
        Self::enter_at(crate::exception::frame_mark(), owner)
    }

    fn enter_at(mark: usize, owner: usize) -> Option<Self> {
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
            let segment_depth = stack.len() - segment_start;
            if segment_depth >= MAX_PROTOTYPE_RESOLUTION_DEPTH
                || stack
                    .iter()
                    .skip(segment_start)
                    .any(|&bits| bits == owner_bits)
            {
                return None;
            }
            let depth_before = stack.len();
            crate::gc::runtime_write_barrier_root_nanbox(owner_bits);
            stack.push(mark, owner_bits);
            Some(Self {
                depth_before,
                segment_depth,
            })
        })
    }
}

impl Drop for PrototypeResolutionGuard {
    fn drop(&mut self) {
        PROTOTYPE_RESOLUTION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            // A landing pad may already have popped this entry (its frame
            // was abandoned). Cleanup after that restore must not pop a
            // still-live outer resolution entry.
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
/// the catching landing pad, which pops the boundary by its frame mark.
pub(crate) struct UserCodeResolutionBoundary {
    depth_before: Option<usize>,
}

impl UserCodeResolutionBoundary {
    #[inline(always)]
    pub(crate) fn enter() -> Self {
        let mark = crate::exception::frame_mark();
        let depth_before = PROTOTYPE_RESOLUTION_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let depth = stack.len();
            (depth != 0).then(|| {
                stack.push(mark, USER_CODE_BOUNDARY);
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

pub(crate) fn resolution_stack_depth() -> usize {
    PROTOTYPE_RESOLUTION_STACK.with(|stack| stack.borrow().len())
}

pub(crate) fn resolution_stack_unwind_to(sp: usize) {
    PROTOTYPE_RESOLUTION_STACK.with(|stack| {
        stack.borrow_mut().unwind_to(sp);
    });
}

pub(crate) fn resolution_stack_check_marks(sp: usize) {
    PROTOTYPE_RESOLUTION_STACK.with(|stack| {
        crate::exception::marks::check_marks(stack.borrow().marks(), sp, "prototype_resolution")
    });
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
pub(crate) fn test_resolution_stack_enter_and_forget(owner: usize, mark: usize) -> bool {
    let Some(guard) = PrototypeResolutionGuard::enter_at(mark, owner) else {
        return false;
    };
    std::mem::forget(guard);
    true
}

/// #6759 Phase B: classify `obj_ptr` as a genuine shaped `GC_TYPE_OBJECT`
/// whose shape can name its prototype directly. Other public owners answer
/// from their own property holder instead.
pub(crate) unsafe fn meta_capable_object(obj_ptr: usize) -> Option<*mut crate::ObjectHeader> {
    (receiver_header_type(obj_ptr)? == crate::gc::GC_TYPE_OBJECT)
        .then_some(obj_ptr as *mut crate::ObjectHeader)
}

/// The type byte of a receiver word that can own a prototype record, or
/// `None` for handles and non-heap bits. The type byte alone decides between
/// a shaped object (GC_TYPE_OBJECT) and a byte cell (Buffer, TypedArray,
/// ArrayBuffer, DataView carry byte-family ids), so one header load serves
/// both.
#[inline]
unsafe fn receiver_header_type(obj_ptr: usize) -> Option<u8> {
    if !crate::value::addr_class::is_above_handle_band(obj_ptr)
        || !crate::object::is_valid_obj_ptr(obj_ptr as *const u8)
    {
        return None;
    }
    Some(crate::value::addr_class::try_read_gc_header(obj_ptr)?.obj_type)
}

/// Public heap receivers whose own-property holder can record a prototype.
pub(crate) fn owned_prototype_receiver_type(kind: u8) -> bool {
    crate::gc::is_byte_family_type(kind)
        || matches!(
            kind,
            crate::gc::GC_TYPE_ARRAY
                | crate::gc::GC_TYPE_OBJECT
                | crate::gc::GC_TYPE_CLOSURE
                | crate::gc::GC_TYPE_PROMISE
                | crate::gc::GC_TYPE_ERROR
                | crate::gc::GC_TYPE_MAP
                | crate::gc::GC_TYPE_LAZY_ARRAY
                | crate::gc::GC_TYPE_SET
                | crate::gc::GC_TYPE_DATE_CELL
                | crate::gc::GC_TYPE_TEMPORAL
        )
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
    // A family instance moves, like every receiver, to the shape naming the
    // link: the evaluation's copy of its family layout (the same keys and
    // slots at `MIXED | class | serial`, `layout_desc::family_desc_of`), so
    // shape-proved readers answer from the evaluation's prototype.
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::ClassEvaluation)
}

/// [`object_link_class_evaluation_prototype`] of a class evaluation's fresh
/// instance that also takes the evaluation's holder brand `holder_brand`
/// (`shapes::prepare_evaluation_holder`) in the same shape transition. A
/// receiver whose prototype is no shape fact takes the link alone; the
/// caller then stamps the brand as usual (`shapes::stamp_evaluation_holder`,
/// a no-op on a receiver that has it).
pub(crate) fn object_link_class_evaluation_prototype_branded(
    obj_ptr: usize,
    proto_bits: u64,
    holder_brand: u64,
) {
    object_set_static_prototype_impl_with(
        obj_ptr,
        proto_bits,
        PrototypeLinkKind::ClassEvaluation,
        holder_brand,
    )
}

/// Install Object.create's individual chain before the object escapes. It
/// needs the user-override dispatch guards, but cannot invalidate any existing
/// receiver's store plans or element-shape proofs.
pub(crate) fn object_link_created_prototype(obj_ptr: usize, proto_bits: u64) {
    object_set_static_prototype_impl(obj_ptr, proto_bits, PrototypeLinkKind::FreshObject)
}

fn object_set_static_prototype_impl(obj_ptr: usize, proto_bits: u64, link_kind: PrototypeLinkKind) {
    object_set_static_prototype_impl_with(obj_ptr, proto_bits, link_kind, 0)
}

/// [`object_set_static_prototype_impl`] whose shape transition also gives the
/// receiver evaluation holder brand `holder_brand` (0: none).
fn object_set_static_prototype_impl_with(
    obj_ptr: usize,
    proto_bits: u64,
    link_kind: PrototypeLinkKind,
    holder_brand: u64,
) {
    let user_override = matches!(
        link_kind,
        PrototypeLinkKind::UserOverride | PrototypeLinkKind::FreshObject
    );
    if obj_ptr == 0 {
        return;
    }
    // #11915: the program relinking a sealed builtin prototype's own
    // [[Prototype]] (`Object.setPrototypeOf`, `__proto__`). Runtime wiring
    // and internal construction (realm population, lazy intrinsics, the
    // `%Array.prototype%` named-property bag linked to `%Object.prototype%`
    // when the first array takes a named property) are the runtime's own
    // definitions and do not count.
    if matches!(link_kind, PrototypeLinkKind::UserOverride) {
        super::builtin_proto::note_write_addr(obj_ptr, "[[Prototype]] link");
        // Object model step 3 (§3.6): every chain a binding site proved
        // through this object changed.
        super::shadow::note_relink(obj_ptr);
    }
    crate::typedarray_named::note_prototype_mutation(obj_ptr, user_override);
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
    let prototype_handle = scope.root_nanbox_u64(proto_bits);
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
    let mut obj_ptr = obj_ptr as usize;
    if obj_ptr >= crate::gc::GC_HEADER_SIZE + 0x1000
        && crate::value::addr_class::is_above_handle_band(obj_ptr)
        && crate::object::is_valid_obj_ptr(obj_ptr as *const u8)
    {
        let obj_type = unsafe {
            let hdr =
                (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
            (*hdr).obj_type
        };
        if obj_type == crate::gc::GC_TYPE_ARRAY {
            // #10593: a retargeted ordinary array is a PER-ARRAY fact. The
            // owner sets `GC_ARRAY_CUSTOM_PROTO` on its own header, and
            // every inline element guard tests that
            // bit next to the global byte, so only THIS array leaves the fast
            // path. The runtime latch stays: it only gates the slow paths'
            // per-array property-bag probe.
            ARRAY_TARGET_PROTO_RECORDED.store(true, Ordering::Relaxed);
            // `Array.prototype` itself heads every default chain, so a new
            // `[[Prototype]]` there IS global. Resolving its address can
            // bootstrap `globalThis` (allocation), so do it with the owner
            // rooted and reload the owner afterwards.
            let (array_prototype, owner) =
                owner_handle.across_mut::<u8, _>(crate::array::array_prototype_addr);
            obj_ptr = owner as usize;
            unsafe {
                let header =
                    crate::gc::header_from_trusted_user_ptr(obj_ptr as *const u8).cast_mut();
                (*header)._reserved |= crate::gc::GC_ARRAY_CUSTOM_PROTO;
            }
            // #11915: this array no longer reads `%Array.prototype%`; the
            // runtime dispatcher's one-load early exit must stop answering
            // for it.
            crate::object::builtin_proto::note_array_custom_proto();
            if array_prototype != 0 && array_prototype == obj_ptr {
                crate::array::invalidate_array_index_fast_path();
                // Every array's chain runs through `%Array.prototype%`'s
                // holder: its shadow bit now says the chain left its layout.
                crate::object::builtin_proto::note_array_prototype_write(obj_ptr, "relink");
            }
        } else if obj_type == crate::gc::GC_TYPE_LAZY_ARRAY {
            // A lazy JSON array's materialized storage is a SEPARATE
            // `GC_TYPE_ARRAY` allocation that does not carry the owner's bit,
            // so keep the conservative process-wide invalidation here.
            ARRAY_TARGET_PROTO_RECORDED.store(true, Ordering::Relaxed);
            crate::array::invalidate_array_index_fast_path();
            crate::object::builtin_proto::note_array_custom_proto();
        }
    }
    let obj_ptr = obj_ptr;
    let proto_bits = prototype_handle.get_nanbox_u64();
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
    // A shaped object's [[Prototype]] is a fact of its SHAPE: the receiver
    // moves to the shape naming its new prototype, and that shape record's
    // `prototype` word is what every reader returns (`shapes_prototype`).
    // Only a receiver whose shape answers nothing about it
    // (`PROTO_ID_PER_OBJECT`: a native-module namespace, an exotic-read
    // receiver) records it in its meta record. Other owners use their property holder below.
    unsafe {
        if crate::buffer::header::is_owned_byte_cell(obj_ptr) {
            crate::buffer::store::bag_set(
                obj_ptr,
                crate::buffer::store::PROTOTYPE_KEY,
                f64::from_bits(proto_bits),
                true,
            );
            return;
        }
        if let Some(obj) = meta_capable_object(obj_ptr) {
            let scope = crate::gc::RuntimeHandleScope::new();
            let obj_handle = scope.root_raw_mut_ptr(obj);
            let proto_handle = scope.root_nanbox_u64(proto_bits);
            // The identity reads the receiver's class and classification and
            // the prototype's serial — never this receiver's meta word — so
            // it is taken before anything allocates. A prototype with no
            // serial (a function, array or typed array) gets an identity of
            // its own. Same predecessor + same prototype reaches the same
            // shape, so construction shares shapes as before; a class-default
            // link is not exempt (`F.prototype = other` then `new F()` must
            // not leave old and new instances on one shape over two chains).
            let proto_id = match prototype_serial {
                Some(_) => crate::object::shapes::object_proto_id_for(obj, proto_bits),
                None => crate::object::shapes::fresh_unique_proto_id(),
            };
            // A receiver whose shape word cannot be written cannot move to a
            // shape naming the prototype either: its meta record is the
            // authority, as for a per-object identity.
            let per_object = proto_id == crate::object::shapes::PROTO_ID_PER_OBJECT
                || !crate::object::shapes::shape_word_is_writable(obj);
            let mut obj = obj;
            // The prototype is a fact of the receiver's shape
            // (`shapes::object_prototype_word`), so a link allocates no meta
            // record. A receiver that has one anyway, or whose PER_OBJECT
            // identity answers nothing from its shape, keeps the same bits in
            // it: the cheaper read, written here and nowhere else.
            if per_object || !(*obj).meta.is_null() {
                // `object_meta_ensure` allocates and may evacuate the owner.
                let (meta, moved) = obj_handle.across_mut::<crate::object::ObjectHeader, _>(|| {
                    crate::object::object_meta_ensure(obj)
                });
                obj = moved;
                let word = proto_handle.get_nanbox_u64();
                (*meta).prototype = word;
                // GC_STORE_AUDIT(BARRIERED): meta-record prototype slot store —
                // the record is an arena allocation, so the ordinary
                // object-slot barrier applies (parent = the meta record).
                crate::gc::runtime_write_barrier_slot(
                    meta as usize,
                    &(*meta).prototype as *const u64 as usize,
                    word,
                );
            }
            let proto_bits = proto_handle.get_nanbox_u64();
            #[cfg(feature = "shape-mint-diag")]
            if link_kind != PrototypeLinkKind::ClassDefault {
                crate::object::shape_mint_census::note_proto_divergence(
                    crate::object::shapes::object_shape_stamp(obj),
                    proto_bits,
                );
            }
            crate::object::shapes::transition_object_shape_prototype_with_holder(
                obj,
                proto_id,
                proto_bits,
                holder_brand,
            );
            // The receiver may already be traced by an incremental mark; its
            // new edge lives in its shape record, outside the heap.
            crate::gc::runtime_shade_external_edge(proto_bits);
            return;
        }
    }
    // The owner already supplies its property holder through an ordinary
    // traced edge. Allocation is bounded to publishing that private bag/meta;
    // no JS runs in this no-move window.
    let _no_move = crate::gc::GcSuppressScope::new();
    let _edit = crate::object::descriptor_state::HolderEdit::new(obj_ptr);
    unsafe {
        let holder = crate::object::descriptor_state::descriptor_holder(obj_ptr);
        if !holder.is_null() {
            let meta = crate::object::object_meta_ensure(holder);
            // GC_STORE_AUDIT(BARRIERED): the bag owns this NaN-boxed prototype
            // edge; the exact metadata-slot barrier follows the store.
            (*meta).prototype = proto_bits;
            crate::gc::runtime_write_barrier_slot(
                meta as usize,
                std::ptr::addr_of!((*meta).prototype) as usize,
                proto_bits,
            );
        }
    }
}

/// Look up the recorded prototype bits for an object, if any. Returns `None`
/// when no explicit prototype has been recorded (the object still has its
/// default prototype); `Some(TAG_NULL)` when it was explicitly set to `null`.
pub fn object_static_prototype(obj_ptr: usize) -> Option<u64> {
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_decoded_pointer(obj_ptr);
    }
    // A shaped object answers from its shape record (or, for a per-object
    // identity, its meta record) — no global latch, no mutex — and NEVER has
    // a residual registry entry (the write path classifies identically), so
    // a miss for a shaped object is authoritative.
    unsafe {
        match receiver_header_type(obj_ptr) {
            Some(crate::gc::GC_TYPE_OBJECT) => {
                let obj = obj_ptr as *mut crate::ObjectHeader;
                let bits = crate::object::shapes::object_prototype_word(obj);
                return (bits != 0).then_some(bits);
            }
            // A byte cell keeps a custom prototype in its bag. The words this
            // reads come from arbitrary receivers, and a byte-family type byte
            // also appears at the start of non-GC memory, so the bag is read
            // only for an allocator-proven cell. Ordinary objects never get here.
            Some(obj_type)
                if crate::gc::is_byte_family_type(obj_type)
                    && crate::buffer::header::byte_cell_is_owned(obj_ptr, obj_type) =>
            {
                return crate::buffer::store::bag_get(obj_ptr, crate::buffer::store::PROTOTYPE_KEY)
                    .map(f64::to_bits);
            }
            Some(kind)
                if !owned_prototype_receiver_type(kind)
                    && crate::value::addr_class::try_read_tracked_gc_header(obj_ptr).is_some() =>
            {
                return None;
            }
            _ => {}
        }
    }
    unsafe {
        let holder = crate::object::descriptor_state::descriptor_holder(obj_ptr);
        if holder.is_null() || (*holder).meta.is_null() {
            return None;
        }
        let bits = (*(*holder).meta).prototype;
        (bits != 0).then_some(bits)
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
#[cfg(test)]
pub(crate) fn prototype_chain_ends_in_explicit_null(obj_ptr: usize) -> bool {
    chain_ends_in_explicit_null_before::<false>(obj_ptr)
}

/// `x instanceof Object` for a shaped receiver: OrdinaryHasInstance finds
/// `Object.prototype` on the chain unless the chain ends in an explicit null
/// first (`Object.create(null)`, `__proto__: null`, a hop re-prototyped to
/// null).
/// A recorded link cannot name an intrinsic that has never been built, so
/// observe the existing root without allocating it just to classify a miss.
pub(crate) fn prototype_chain_ends_in_null_before_object_prototype(obj_ptr: usize) -> bool {
    chain_ends_in_explicit_null_before::<true>(obj_ptr)
}

/// Walk recorded/class links. The canonical intrinsic is itself born null;
/// distinguish its edge only when a null edge is actually reached. An
/// ordinary unrecorded miss needs no intrinsic lookup or bootstrap work.
fn chain_ends_in_explicit_null_before<const STOP_AT_OBJECT_PROTOTYPE: bool>(
    obj_ptr: usize,
) -> bool {
    let mut current = obj_ptr;
    // The same bound the generic chain walk uses. A cycle cannot be built
    // through `setPrototypeOf` (it refuses one), but a bound is cheaper than
    // trusting that from here.
    for _ in 0..32 {
        match object_static_prototype(current) {
            // A cell born with no prototype ends the chain unless a later
            // link recorded one (the born-null header bit is sticky).
            None if unsafe { cell_is_born_null_proto(current) } => {
                return !STOP_AT_OBJECT_PROTOTYPE
                    || current != crate::array::object_prototype_addr_if_resolved();
            }
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
            Some(TAG_NULL) => {
                return !STOP_AT_OBJECT_PROTOTYPE
                    || current != crate::array::object_prototype_addr_if_resolved();
            }
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
    super::class_registry::class_holder_prototype(class_id) as usize
}

/// Was this cell allocated with no prototype (`Object.create(null)`,
/// `querystring.parse`)? That is `OBJ_FLAG_NULL_PROTO`, the header bit #1175
/// added, and it is the born-null half of the question
/// [`chain_ends_in_explicit_null_before`] asks.
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

/// Whether ordinary property lookup must consult the receiver's own chain
/// before its class's shared surface (vtable, declaration prototype): its
/// shape's prototype identity is not the one its class implies — a user
/// `setPrototypeOf`/`__proto__`, an evaluated class's prototype, runtime
/// wiring to anything but the class's own declaration prototype. A shape
/// read, not a flag. An explicit null edge is authoritative even without a
/// declared class surface; otherwise a class-id-zero receiver has no class
/// surface for its chain to replace.
#[inline]
pub(crate) fn object_has_individual_class_prototype(obj_ptr: usize) -> bool {
    unsafe {
        let Some(obj) = meta_capable_object(obj_ptr) else {
            return false;
        };
        let word = (*obj).shape_word();
        // Plain shapes cannot replace a class surface. Preserve this
        // header-only admission before validating a linked ShapeId.
        if !crate::object::shapes::shape_word_may_be_linked(word) {
            return false;
        }
        let stamp = crate::object::shapes::object_shape_stamp(obj);
        // The null identity is already encoded in the ShapeId. Read that
        // fact before the class eligibility guard, without fetching a slab.
        if crate::object::shapes::shape_word_kind(stamp.0)
            == crate::object::shapes::SHAPE_ID_KIND_NULL
        {
            return true;
        }
        // A non-class receiver has no shared class surface to supersede.
        // This eligibility guard does not select any default prototype.
        if (*obj).class_id == 0 {
            return false;
        }
        let pid = crate::object::shapes::object_proto_identity(obj);
        // CLASS is already the selected origin, DEFAULT/serial describe
        // ordinary chains. A MIXED identity carries a declared class plus
        // its evaluation/override link. No registry re-derives these facts.
        pid >= crate::object::shapes::PROTO_ID_MIXED
            && pid != crate::object::shapes::PROTO_ID_PER_OBJECT
    }
}

/// Does this receiver stand on a prototype from OUTSIDE its own class — not
/// its class's declaration prototype, nor an evaluation prototype of its own
/// (template) class? A user `setPrototypeOf`/`__proto__`, `Object.create`,
/// or a `null` link. A fact of what the receiver's shape names (its recorded
/// prototype's identity), not of how it was linked: re-pointing it back to
/// its class's prototype makes it false again. Then the declared class ids
/// do not describe the receiver's chain at all.
pub(crate) fn object_prototype_is_foreign(obj_ptr: usize) -> bool {
    unsafe {
        let Some(obj) = meta_capable_object(obj_ptr) else {
            return false;
        };
        let recorded = crate::object::shapes::object_prototype_word(obj);
        if recorded == 0 {
            return false;
        }
        // A class object's [[Prototype]] is its heritage (the parent
        // constructor): its static chain, which the static walks read from
        // its recorded link.
        if crate::object::class_registry::is_class_object_ptr(obj.cast()) {
            return false;
        }
        let value = crate::value::JSValue::from_bits(recorded);
        if !value.is_pointer() {
            return true;
        }
        let proto = value.as_pointer::<crate::ObjectHeader>();
        let Some(header) = crate::value::addr_class::try_read_gc_header(proto as usize) else {
            return true;
        };
        let class_id = (*obj).class_id;
        !(header.obj_type == crate::gc::GC_TYPE_OBJECT
            && class_id != 0
            && (*proto).class_id == class_id)
    }
}

/// #11391: `new F()` records F's `.prototype` of that moment as the instance's
/// `[[Prototype]]` (a class-default link, so no override flag) and also stamps
/// F's synthetic class id. The class-id walk reads F's CURRENT `.prototype`
/// (`CLASS_PROTOTYPE_OBJECTS[F]`). After `F.prototype = other` the two name
/// different objects, and only the recorded one is on the instance's chain:
/// `o.b` answered from `other`, an object `o` does not inherit from.
///
/// True exactly when they differ. When they name the same object the class-id
/// walk reads the right one, and its arms (decl-proto accessors, evaluated
/// parents) stay in charge. Declared class ids are excluded by
/// `synthetic_class_prototype_object`: their entry is a parent class object,
/// never a prototype, so the comparison would be meaningless.
pub(crate) fn class_default_prototype_superseded(obj_ptr: usize) -> bool {
    let Some(recorded) = object_static_prototype(obj_ptr) else {
        return false;
    };
    // Only an ordinary object carries a class id at the `ObjectHeader` offset.
    let Some(obj) = (unsafe { meta_capable_object(obj_ptr) }) else {
        return false;
    };
    let class_id = unsafe { (*obj).class_id };
    if class_id == 0 {
        return false;
    }
    let recorded = crate::value::JSValue::from_bits(recorded);
    if !recorded.is_pointer() {
        return false;
    }
    let current = crate::object::class_registry::synthetic_class_prototype_object(class_id);
    !current.is_null() && current as usize != recorded.as_pointer::<u8>() as usize
}

pub(crate) fn default_object_prototype_bits() -> Option<u64> {
    crate::object::object_prototype_intrinsic_bits()
}

pub(crate) unsafe fn default_object_prototype_for_owner(obj_ptr: usize) -> Option<u64> {
    if obj_ptr == 0 {
        return None;
    }
    let obj = obj_ptr as *const crate::ObjectHeader;
    if !crate::value::addr_class::is_plausible_heap_addr(obj_ptr) {
        return None;
    }
    let gc = super::gc_header_for(obj);
    if (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0 {
        return None;
    }
    if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT
        || ((*obj).class_id != 0 && !super::shapes::object_is_plain_record(obj))
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
    let guard = PrototypeResolutionGuard::enter(obj_ptr)?;
    if proto_bits == TAG_NULL {
        return None;
    }
    // A Proxy prototype (`Object.create(proxy).x`) is a Proxy cell, not an
    // ObjectHeader. Route the inherited read through the proxy's `[[Get]]`
    // (which fires the get trap or forwards to the target), binding the
    // original instance as the receiver. (test262
    // Proxy/get/trap-is-{null,undefined}-target-is-proxy via
    // `Object.create(proxy)[k]`.)
    {
        let proto_val = f64::from_bits(proto_bits);
        if crate::proxy_hooks::proxy_is_proxy(proto_val) != 0 {
            if key.is_null() {
                return None;
            }
            let key_val = f64::from_bits(crate::value::js_nanbox_string(key as i64).to_bits());
            let receiver = super::field_get_set::accessor_receiver_override_take()
                .unwrap_or_else(|| crate::value::js_nanbox_pointer(obj_ptr as i64));
            let v = crate::proxy_hooks::proxy_get_from_prototype(proto_val, key_val, receiver);
            return Some(crate::value::JSValue::from_bits(v.to_bits()));
        }
    }
    let top16 = proto_bits >> 48;
    let proto_ptr = if top16 == 0x7FFD {
        (proto_bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if top16 == 0 && crate::value::addr_class::is_above_handle_band(proto_bits as usize) {
        proto_bits as usize
    } else {
        return None;
    };
    if proto_ptr == 0 || proto_ptr == obj_ptr {
        return None;
    }
    let mut proto = proto_ptr as *const crate::ObjectHeader;
    if !crate::value::addr_class::is_plausible_heap_addr(proto_ptr) {
        return None;
    }
    if guard.segment_depth >= SKIP_PROVEN_ABSENT_FROM_DEPTH && !key.is_null() {
        // SAFETY: a live key string and a validated object pointer; the skip
        // makes the same shape reads `in` makes and runs no user code.
        let holder = unsafe {
            let name = std::slice::from_raw_parts(
                (key as *const u8).add(std::mem::size_of::<crate::StringHeader>()),
                (*key).byte_len as usize,
            );
            super::field_get_set::has_property::shape_presence::first_possible_holder(proto, name)
        };
        match holder {
            super::field_get_set::has_property::shape_presence::ChainHolder::At(holder) => {
                proto = holder
            }
            super::field_get_set::has_property::shape_presence::ChainHolder::Absent => return None,
        }
        if proto as usize == obj_ptr
            || !crate::value::addr_class::is_plausible_heap_addr(proto as usize)
        {
            return None;
        }
    }
    // `js_object_get_field_by_name` handles its own further prototype hops
    // (recorded protos on the proto object), so this is the full walk.
    let receiver = f64::from_bits(crate::value::js_nanbox_pointer(obj_ptr as i64).to_bits());
    let scope = crate::gc::RuntimeHandleScope::new();
    // The recursive `get_field(proto, key)` re-derives the accessor receiver
    // from `proto`; stash the real instance so an inherited getter binds `this`
    // to it, not to the prototype.
    let prev_override = super::field_get_set::accessor_receiver_override_begin(receiver);
    let prev_override_handle = prev_override.map(|value| scope.root_nanbox_f64(value));
    let v = super::js_object_get_field_by_name(proto, key);
    super::field_get_set::accessor_receiver_override_end(
        prev_override_handle.map(crate::gc::RuntimeHandle::into_nanbox_f64),
    );
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
/// tests must restore the process flag after their owner becomes unreachable. A test
/// restores what it found; see `ArrayPrototypeLatchGuard` in
/// `dyn_eval/tests.rs`.
#[cfg(test)]
pub(crate) fn test_swap_array_static_proto_recorded(value: bool) -> bool {
    ARRAY_TARGET_PROTO_RECORDED.swap(value, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_wiring_and_user_override_publish_distinct_signals() {
        let _no_move = crate::gc::GcSuppressScope::new();

        let runtime_wired = crate::object::js_object_alloc(0, 0);
        object_set_static_prototype(runtime_wired as usize, crate::value::TAG_NULL);
        // A diverged prototype is a different shape, not a flag.
        assert_eq!(
            object_static_prototype(runtime_wired as usize),
            Some(crate::value::TAG_NULL)
        );

        let class_default = crate::object::js_object_alloc(0, 0);
        object_link_class_default_prototype(class_default as usize, crate::value::TAG_NULL);
        // The prototype is a shape fact, and a link records no flag: the
        // receiver needs no meta record at all.
        assert!(unsafe { (*class_default).meta }.is_null());
        assert_eq!(
            object_static_prototype(class_default as usize),
            Some(crate::value::TAG_NULL)
        );

        // "Individual" is read from the shape: a compiled class's instance
        // whose identity names anything but its class's prototype. Receivers
        // with no class surface still carry an authoritative explicit null edge.
        const CLASS: u32 = 0x7A;
        let fresh = crate::object::js_object_alloc(CLASS, 0);
        assert!(!object_has_individual_class_prototype(fresh as usize));
        let evaluated = crate::object::js_object_alloc(CLASS, 0);
        object_link_class_evaluation_prototype(evaluated as usize, crate::value::TAG_NULL);
        assert!(object_has_individual_class_prototype(evaluated as usize));
        assert!(unsafe { (*evaluated).meta }.is_null(), "no flag, no record");
        assert!(object_has_individual_class_prototype(
            class_default as usize
        ));
        assert!(object_has_individual_class_prototype(
            runtime_wired as usize
        ));

        let user_overridden = crate::object::js_object_alloc(CLASS, 0);
        object_set_user_prototype(user_overridden as usize, crate::value::TAG_NULL);
        assert!(object_has_individual_class_prototype(
            user_overridden as usize
        ));
        assert!(
            unsafe { (*user_overridden).meta }.is_null(),
            "no flag, no record"
        );
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
    fn a_landing_pad_restores_the_resolution_stack() {
        let base_depth = resolution_stack_depth();
        let here = crate::exception::frame_mark();
        let first = PrototypeResolutionGuard::enter(usize::MAX - 1).unwrap();
        let second = PrototypeResolutionGuard::enter(usize::MAX).unwrap();
        assert_eq!(resolution_stack_depth(), base_depth + 2);

        // A throw skips these drops. Forget the guards to model that, then
        // replay the restore of a landing pad in this frame's caller.
        std::mem::forget(first);
        std::mem::forget(second);
        resolution_stack_unwind_to(here + 1);

        assert_eq!(resolution_stack_depth(), base_depth);
    }

    #[test]
    fn system_unwind_drop_is_idempotent_after_resolution_restore() {
        let base_depth = resolution_stack_depth();
        let here = crate::exception::frame_mark();
        let first = PrototypeResolutionGuard::enter(usize::MAX - 1).unwrap();
        let second = PrototypeResolutionGuard::enter(usize::MAX).unwrap();
        assert_eq!(resolution_stack_depth(), base_depth + 2);

        resolution_stack_unwind_to(here + 1);
        drop(second);
        drop(first);

        assert_eq!(resolution_stack_depth(), base_depth);
    }
}

#[cfg(test)]
#[path = "proxy_reentry_tests.rs"]
mod proxy_reentry_tests;

#[cfg(test)]
mod readpath_null_tests {
    #[test]
    fn nonclass_link_keeps_ordinary_reads_without_a_class_surface() {
        use crate::object::{self, prototype_chain as chain};
        let _no_move = crate::gc::GcSuppressScope::new();
        let prototype = object::js_object_alloc(0, 0);
        let receiver = object::js_object_alloc(0, 0);
        chain::object_set_user_prototype(
            receiver as usize,
            crate::value::js_nanbox_pointer(prototype as i64).to_bits(),
        );
        assert!(!chain::object_has_individual_class_prototype(
            receiver as usize
        ));
        let key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        object::js_object_set_field_by_name(prototype, key, 42.0);
        assert_eq!(
            object::js_object_get_field_by_name(receiver, key).as_number(),
            42.0
        );
        chain::object_set_user_prototype(receiver as usize, crate::value::TAG_NULL);
        assert!(chain::object_has_individual_class_prototype(
            receiver as usize
        ));
        assert!(object::js_object_get_field_by_name(receiver, key).is_undefined());
    }

    #[test]
    fn synthetic_instance_null_edge_overrides_constructor_surface() {
        use crate::object::{self, prototype_chain as chain};
        let _no_move = crate::gc::GcSuppressScope::new();
        let cid = object::class_registry::prototype_objects::alloc_synthetic_class_id();
        let prototype = object::js_object_alloc(0, 0);
        object::class_registry::class_prototype_object_root_store(cid, prototype);
        let instance = object::js_object_alloc(cid, 0);
        chain::object_link_class_default_prototype(
            instance as usize,
            crate::value::js_nanbox_pointer(prototype as i64).to_bits(),
        );
        chain::object_set_user_prototype(instance as usize, crate::value::TAG_NULL);
        assert!(
            chain::object_has_individual_class_prototype(instance as usize),
            "the live null edge must be authoritative even for a synthetic class"
        );
        let key = crate::string::js_string_from_bytes(b"k".as_ptr(), 1);
        {
            object::js_object_set_field_by_name(prototype, key, 42.0);
            assert!(object::js_object_get_field_by_name(instance, key).is_undefined());
        }
    }
}
