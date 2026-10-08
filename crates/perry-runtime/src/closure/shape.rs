//! Every function object carries a real ShapeId (the every-receiver-shape
//! lane, stage 1).
//!
//! A closure's word at payload +4 — the same word an `ObjectHeader` keeps its
//! ShapeId in — names:
//!
//! * [`ShapeObjectKind::Function`] with the prototype its body kind implies
//!   (`Function.prototype`, `%AsyncFunction.prototype%`,
//!   `%GeneratorFunction.prototype%`, `%AsyncGeneratorFunction.prototype%`),
//!   while the closure's own properties are exactly the intrinsic ones
//!   (`name`, `length`, `prototype`) and nothing recorded a [[Prototype]];
//! * [`ShapeObjectKind::FunctionDictionary`] once anything else was installed:
//!   the answer then lives on the object (its side tables), exactly as a
//!   dictionary-mode `ObjectHeader`'s does.
//!
//! Both are minted in the exotic band (`shapes::EXOTIC_SHAPE_ID_BASE`), which
//! no own-inline-slot site word accepts, so no emitted cache can ever load
//! `closure + 16 + 8*slot` as if it were an object slot.
//!
//! The transition is one-way and happens at the funnels that change what a
//! closure answers: an own-property install (`closure_set_dynamic_prop`), a
//! delete, an accessor/descriptor install, a recorded [[Prototype]]. The kind
//! of a cell is its GC type byte — never a magic word in its payload.
use super::ClosureHeader;
use crate::object::shapes::{self, ShapeObjectKind};

/// Intrinsic-prototype serials (`ObjectMeta.proto_serial`) assigned at
/// creation, so a base shape can name its prototype before the prototype
/// object exists. Dynamic serials start above
/// [`crate::object::proto_validity::FIRST_DYNAMIC_PROTOTYPE_SERIAL`].
pub(crate) const INTRINSIC_SERIAL_FUNCTION: u64 = 1;
pub(crate) const INTRINSIC_SERIAL_ASYNC_FUNCTION: u64 = 2;
pub(crate) const INTRINSIC_SERIAL_GENERATOR_FUNCTION: u64 = 3;
pub(crate) const INTRINSIC_SERIAL_ASYNC_GENERATOR_FUNCTION: u64 = 4;
/// Not a prototype: the marker `proto_id` of the class-constructor ShapeId
/// ([`function_class_shape`]). Shapes are canonical per facts, so without a
/// fact of its own the class shape would BE the FunctionDictionary id. No
/// object is ever assigned this serial; a class constructor's real
/// [[Prototype]] lives on the object (dictionary kind: "ask the object").
pub(crate) const INTRINSIC_SERIAL_CLASS_CONSTRUCTOR_MARKER: u64 = 5;

/// Which intrinsic prototype a function BODY's closures inherit from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub(crate) enum FunctionProtoKind {
    Function = 0,
    AsyncFunction = 1,
    Generator = 2,
    AsyncGenerator = 3,
}

impl FunctionProtoKind {
    fn serial(self) -> u64 {
        match self {
            FunctionProtoKind::Function => INTRINSIC_SERIAL_FUNCTION,
            FunctionProtoKind::AsyncFunction => INTRINSIC_SERIAL_ASYNC_FUNCTION,
            FunctionProtoKind::Generator => INTRINSIC_SERIAL_GENERATOR_FUNCTION,
            FunctionProtoKind::AsyncGenerator => INTRINSIC_SERIAL_ASYNC_GENERATOR_FUNCTION,
        }
    }

    /// The body kind `info` records — the same bits
    /// `generator_function_proto_of` answers [[GetPrototypeOf]] from.
    pub(crate) fn of_body(info: &super::JsFunctionInfo) -> FunctionProtoKind {
        if info.flags & super::FN_ASYNC_GENERATOR != 0 {
            FunctionProtoKind::AsyncGenerator
        } else if info.flags & super::FN_GENERATOR != 0 {
            FunctionProtoKind::Generator
        } else if info.flags & super::FN_ASYNC != 0 {
            FunctionProtoKind::AsyncFunction
        } else {
            FunctionProtoKind::Function
        }
    }
}

crate::perry_thread_local! {
    static FUNCTION_PROTOTYPE_SLOT: std::sync::atomic::AtomicI64 =
        const { std::sync::atomic::AtomicI64::new(0) };
}

/// This agent's `Function.prototype` — the object the base Function
/// ShapeId's `proto_id` names — published when the global table populates it.
/// A GC pointer in a static: scanned (kept alive AND rewritten on a move) by
/// [`scan_function_prototype_roots_mut`], registered from
/// `object::scan_object_cache_roots_mut`.
pub(crate) static FUNCTION_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&FUNCTION_PROTOTYPE_SLOT);

/// This realm's `%Function.prototype%`, building the realm global (which
/// allocates it) when none exists yet. For a read that continues ON that
/// object: a function's [[Prototype]] exists as soon as the function does,
/// whether or not code has named it yet. [`FUNCTION_PROTOTYPE_PTR`] alone is
/// 0 until then and answers only "has anything been installed there".
/// Allocates on the first call: callers root what they hold across it.
pub(crate) fn function_prototype_ptr_materialized() -> usize {
    let proto = FUNCTION_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire);
    if proto != 0 {
        return proto as usize;
    }
    let _ = crate::object::js_get_global_this();
    FUNCTION_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire) as usize
}

/// GC root for [`FUNCTION_PROTOTYPE_PTR`].
pub(crate) fn scan_function_prototype_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    FUNCTION_PROTOTYPE_PTR.with_slot(|slot| {
        visitor.visit_atomic_i64_slot(
            slot,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Release,
        );
    });
}

crate::perry_thread_local! {
    /// This agent's base Function ShapeIds, indexed by `FunctionProtoKind`,
    /// then the FunctionDictionary id (0 = not minted yet).
    static BASE_SHAPES: std::cell::Cell<[u32; 5]> = const { std::cell::Cell::new([0; 5]) };
    /// This agent's class-constructor ShapeId (0 = not minted yet). Its own
    /// cell: the base-shape array is copied on every closure birth.
    static CLASS_SHAPE: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// The attribute summary a Function shape publishes. Its keys are not in a
/// shared keys array, so the summary cannot be derived from them: the base
/// shape describes the intrinsic `name`/`length`/`prototype`, none of which is
/// a default (writable, enumerable, configurable) data property, and a
/// FunctionDictionary shape may describe accessors too. Reporting them keeps
/// every summary-first reader from treating a function as all-default.
fn function_shape_summary(kind: ShapeObjectKind) -> u8 {
    use crate::object::key_attrs::{
        SUMMARY_ACCESSOR, SUMMARY_NON_CONFIGURABLE, SUMMARY_NON_ENUMERABLE, SUMMARY_NON_WRITABLE,
    };
    let intrinsic = SUMMARY_NON_WRITABLE | SUMMARY_NON_ENUMERABLE | SUMMARY_NON_CONFIGURABLE;
    match kind {
        ShapeObjectKind::FunctionDictionary => intrinsic | SUMMARY_ACCESSOR,
        _ => intrinsic,
    }
}

#[cold]
#[inline(never)]
fn mint(kind: ShapeObjectKind, proto_id: u64) -> u32 {
    let id = shapes::publish_shape_result(shapes::shape_descriptor_ensure_with_generation(
        std::ptr::null(),
        0,
        0,
        0,
        kind,
        proto_id,
        shapes::ReceiverFacts::summary(function_shape_summary(kind)),
    ));
    // A keyless intrinsic shape: rooted for the agent's life, so the
    // post-full-trace prune can never retire an id live closures carry.
    // SAFETY: the descriptor was just published on this agent.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    id
}

#[inline]
fn base_slot(index: usize, kind: ShapeObjectKind, proto_id: u64) -> u32 {
    // One element read in place: every closure birth and every function
    // receiver test asks for one of these ids, and copying the whole array
    // out of the cell per call showed up at ~3% of Zod.
    // SAFETY: a plain `[u32; 5]` read through the agent's own cell; nothing
    // else holds a reference into it.
    let id = BASE_SHAPES.with(|c| unsafe { (*c.as_ptr())[index] });
    if id != 0 {
        return id;
    }
    mint_base_slot(index, kind, proto_id)
}

#[cold]
#[inline(never)]
fn mint_base_slot(index: usize, kind: ShapeObjectKind, proto_id: u64) -> u32 {
    let id = mint(kind, proto_id);
    BASE_SHAPES.with(|c| {
        let mut ids = c.get();
        ids[index] = id;
        c.set(ids);
    });
    id
}

/// The base ShapeId for a closure of `kind`'s bodies.
#[inline]
pub(crate) fn function_base_shape(kind: FunctionProtoKind) -> u32 {
    base_slot(kind as usize, ShapeObjectKind::Function, kind.serial())
}

/// The shared FunctionDictionary ShapeId: "ask the object".
#[inline]
pub(crate) fn function_dictionary_shape() -> u32 {
    base_slot(
        4,
        ShapeObjectKind::FunctionDictionary,
        shapes::PROTO_ID_PER_OBJECT,
    )
}

/// The ShapeId of every class function object (`object::class_value`): its
/// kind is a shape fact. Dictionary-kind ("ask the object": statics, the
/// recorded [[Prototype]], accessors live on the object) and STICKY — no
/// own-property transition moves a class function object off it, so a site
/// that compares ShapeIds tells a class constructor from any other function
/// with that one compare.
#[inline]
pub(crate) fn function_class_shape() -> u32 {
    let mut id = CLASS_SHAPE.with(std::cell::Cell::get);
    if id == 0 {
        id = mint(
            ShapeObjectKind::FunctionDictionary,
            INTRINSIC_SERIAL_CLASS_CONSTRUCTOR_MARKER,
        );
        CLASS_SHAPE.with(|c| c.set(id));
    }
    debug_assert_ne!(
        id,
        function_dictionary_shape(),
        "the class shape is its own id"
    );
    id
}

/// Is `info` the class function object info — i.e. is a closure carrying it a
/// class function object? Equivalent to its ShapeId being
/// [`function_class_shape`] (both are set at mint and never change); used on
/// hot paths where the shape id would need a thread-local read.
#[inline(always)]
pub(crate) fn is_class_info(info: *const super::JsFunctionInfo) -> bool {
    std::ptr::eq(info, &crate::object::class_value::CLASS_CONSTRUCTOR_INFO)
}

/// The ShapeId a fresh closure of the body `info` describes is born with.
#[inline]
pub(crate) fn birth_shape_for_body(info: *const super::JsFunctionInfo) -> u32 {
    // SAFETY: a non-null info is a static one (the allocation entries' contract).
    let kind = match unsafe { info.as_ref() } {
        Some(info) => FunctionProtoKind::of_body(info),
        // A function object with no body: every call through it is refused
        // (`get_valid_info` answers null), and it has the plain Function shape.
        None => FunctionProtoKind::Function,
    };
    function_base_shape(kind)
}

/// Is `closure` on a DESCRIBED Function shape (base or keyed, any body
/// kind) — i.e. not FunctionDictionary? Such a closure has no accessor, no
/// symbol key, no delete marker and no recorded prototype.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline]
pub(crate) unsafe fn closure_on_base_shape(closure: *const ClosureHeader) -> bool {
    // A closure's word is a base Function id or the one FunctionDictionary
    // id (the only transition this stage makes), so one compare answers it.
    let id = (*closure).shape_id;
    debug_assert!(
        id == function_dictionary_shape()
            || id == function_class_shape()
            || shapes::shape_object_kind_by_id(id) == Some(ShapeObjectKind::Function),
        "a closure carries a Function, FunctionDictionary or class shape: {id:#x}"
    );
    // The class shape is sticky and implies the class info,
    // so the info-pointer compare (a link-time constant, no thread-local
    // read) excludes it for free on this hot path.
    id != function_dictionary_shape() && !is_class_info((*closure).info)
}

/// Record that `closure` now answers something its base shape does not:
/// a non-intrinsic own property, a delete, a descriptor, or a recorded
/// [[Prototype]]. Idempotent; the ShapeId word is not a pointer, so the
/// store needs no barrier.
///
/// # Safety
/// `closure` is a live `GC_TYPE_CLOSURE` cell (forwarding already resolved).
#[inline]
pub(crate) unsafe fn closure_become_dictionary(closure: *mut ClosureHeader) {
    let dict = function_dictionary_shape();
    let id = (*closure).shape_id;
    // A class function object keeps its (sticky, dictionary-kind) class shape.
    if id != dict && !is_class_info((*closure).info) {
        // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
        (*closure).shape_id = dict;
    }
}

/// Recompute the closure's ShapeId from its own-property bag after a string
/// key was added or removed: the base Function shape of its body kind while
/// the bag is empty; a KEYED Function shape (the bag's keys, count and inline
/// bound, this body kind's prototype) while the bag is an ordinary tombstone-
/// free object; FunctionDictionary otherwise. FunctionDictionary is sticky —
/// it keeps fast paths conservative for accessors, symbol keys, recorded
/// prototypes and delete markers. Property attributes remain in the bag.
///
/// Keyed Function records are pinned (`RECORD_FLAG_EXTERNAL_CARRIER`): a
/// closure is not a shape carrier the collector notes, so its record must not
/// be pruned while the closure lives. They are canonical per facts, so the
/// set is bounded by the program's distinct function key lists.
pub(crate) fn refresh_closure_shape(ptr: usize) {
    unsafe {
        let closure = ptr as *mut ClosureHeader;
        let dict = function_dictionary_shape();
        if (*closure).shape_id == dict || is_class_info((*closure).info) {
            return;
        }
        if super::props::has_state(ptr) {
            closure_become_dictionary(closure);
            return;
        }
        let base = birth_shape_for_body((*closure).info);
        let bag = super::props::bag_of(ptr);
        let next = if bag.is_null() {
            base
        } else {
            match shapes::object_shape_descriptor(bag) {
                Some(d)
                    if d.object_kind.is_ordinary_layout()
                        && d.hole_count == 0
                        && d.semantic_generation == 0 =>
                {
                    if d.logical_key_count == 0 {
                        base
                    } else {
                        let proto_id =
                            shapes::shape_proto_id(base).unwrap_or(INTRINSIC_SERIAL_FUNCTION);
                        // The keyed id is canonical in exactly these inputs:
                        // when the closure already carries the id they name
                        // (a value store, an attribute edit that changed
                        // nothing), there is nothing to mint.
                        let current = (*closure).shape_id;
                        if current != base
                            && shapes::shape_descriptor_by_id(current).is_some_and(|f| {
                                f.object_kind == ShapeObjectKind::Function
                                    && f.keys == d.keys
                                    && f.logical_key_count == d.logical_key_count
                                    && f.live_inline_slot_count == d.live_inline_slot_count
                                    && f.semantic_generation == 0
                                    && f.proto_id == proto_id
                                    && f.brands() == d.brands()
                            })
                        {
                            return;
                        }
                        let id = shapes::publish_shape_result(
                            shapes::shape_descriptor_ensure_with_generation(
                                d.keys as usize as *const crate::array::ArrayHeader,
                                d.logical_key_count,
                                d.live_inline_slot_count,
                                0,
                                ShapeObjectKind::Function,
                                proto_id,
                                // The bag's brands are the closure's (#11791).
                                shapes::ReceiverFacts::of_descriptor(
                                    &d,
                                    function_shape_summary(ShapeObjectKind::Function),
                                ),
                            ),
                        );
                        shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id));
                        id
                    }
                }
                _ => dict,
            }
        };
        // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
        (*closure).shape_id = next;
    }
}

/// Does the Function ShapeId `id` describe a receiver that inherits `key`
/// from `Function.prototype`? True for a base or keyed (never dictionary)
/// Function shape whose prototype identity is Function.prototype's and whose
/// own key list does not contain `key`.
pub(crate) fn function_shape_inherits_from_function_prototype(id: u32, key: &[u8]) -> bool {
    // The common receiver: no own keys, Function.prototype — one compare.
    if id == function_base_shape(FunctionProtoKind::Function) {
        return true;
    }
    if id == function_dictionary_shape() {
        return false;
    }
    // A keyed shape (the class shape is dictionary-kind: the verdict below
    // answers false for it without a compare of its own): its verdict for the three Function.prototype intrinsics
    // is a fact of the (immutable) ShapeId, cached per agent.
    let bit = match key {
        b"bind" => VERDICT_BIND,
        b"call" => VERDICT_CALL,
        b"apply" => VERDICT_APPLY,
        _ => return keyed_shape_lacks_key(id, key),
    };
    keyed_shape_verdict(id) & bit != 0
}

/// This agent's base `Function` ShapeId and its FunctionDictionary ShapeId,
/// in one read of the cell. An id not minted yet reads 0, which no function
/// object carries.
#[inline]
pub(crate) fn function_base_and_dictionary_shapes() -> (u32, u32) {
    // SAFETY: a plain `[u32; 5]` read through the agent's own cell; nothing
    // else holds a reference into it.
    BASE_SHAPES.with(|c| unsafe {
        let ids = &*c.as_ptr();
        (ids[FunctionProtoKind::Function as usize], ids[4])
    })
}

/// The verdict mask of a keyed ShapeId: whether it is a Function shape over
/// `Function.prototype`, and which of the three intrinsics it lacks as own
/// keys. A fact of the (immutable) ShapeId, cached per agent.
fn keyed_shape_verdict(id: u32) -> u8 {
    let slot = (id as usize).wrapping_mul(0x9E37_79B9) >> 26 & (VERDICT_CACHE_LEN - 1);
    // Index in place: `Cell::get` would copy the whole 64-entry array.
    // SAFETY: this agent's own cell; no reference to it outlives the read.
    let cached = VERDICT_CACHE.with(|c| unsafe { (*c.as_ptr())[slot] });
    if cached.0 == id {
        return cached.1;
    }
    let over_function_prototype = shapes::shape_descriptor_by_id(id).is_some_and(|d| {
        d.object_kind == ShapeObjectKind::Function && d.proto_id == INTRINSIC_SERIAL_FUNCTION
    });
    let mask = if over_function_prototype {
        VERDICT_KNOWN
            | VERDICT_FUNCTION_PROTOTYPE
            | if keyed_shape_lacks_key(id, b"bind") {
                VERDICT_BIND
            } else {
                0
            }
            | if keyed_shape_lacks_key(id, b"call") {
                VERDICT_CALL
            } else {
                0
            }
            | if keyed_shape_lacks_key(id, b"apply") {
                VERDICT_APPLY
            } else {
                0
            }
    } else {
        VERDICT_KNOWN
    };
    // SAFETY: as above; a single-entry store in place.
    VERDICT_CACHE.with(|c| unsafe { (*c.as_ptr())[slot] = (id, mask) });
    mask
}

const VERDICT_KNOWN: u8 = 1;
const VERDICT_BIND: u8 = 2;
const VERDICT_CALL: u8 = 4;
const VERDICT_APPLY: u8 = 8;
const VERDICT_FUNCTION_PROTOTYPE: u8 = 16;
const VERDICT_CACHE_LEN: usize = 64;

crate::perry_thread_local! {
    /// Per-agent cache of keyed Function ShapeIds' verdicts for the
    /// Function.prototype intrinsics (ShapeIds are never reused, so an entry
    /// can only go unused, never wrong).
    static VERDICT_CACHE: std::cell::Cell<[(u32, u8); VERDICT_CACHE_LEN]> =
        const { std::cell::Cell::new([(0, 0); VERDICT_CACHE_LEN]) };
}

/// A keyed Function shape naming Function.prototype whose key list lacks `key`.
fn keyed_shape_lacks_key(id: u32, key: &[u8]) -> bool {
    let Some(descriptor) = shapes::shape_descriptor_by_id(id) else {
        return false;
    };
    if descriptor.object_kind != ShapeObjectKind::Function
        || descriptor.proto_id != INTRINSIC_SERIAL_FUNCTION
    {
        return false;
    }
    if descriptor.keys == 0 || descriptor.logical_key_count == 0 {
        return true;
    }
    // SAFETY: a live slab record's keys array.
    unsafe {
        crate::object::keys_find_slot_by_bytes_resolved(
            descriptor.keys as usize as *const crate::array::ArrayHeader,
            descriptor.logical_key_count,
            key,
        )
        .is_none()
    }
}

/// The function's own `prototype` value, read through its ShapeId (#10507):
/// `Some(Some(v))` the value, `Some(None)` no own `prototype` yet (a base
/// shape: nothing materialized it), `None` the shape does not say (a
/// FunctionDictionary or class function object, or a key outside the inline
/// slots) and the caller asks the bag.
///
/// A keyed Function shape is minted from the bag's ordinary descriptor
/// (`refresh_closure_shape`), so its key list and live inline bound are the
/// bag's: the slot of `prototype` is a fact of the immutable id, cached per
/// agent like the intrinsic verdicts above. `prototype` of a function is a
/// non-configurable data property, so the slot always holds its value.
///
/// # Safety
/// `closure` is a proven, live closure cell.
#[inline]
pub(crate) unsafe fn closure_own_prototype_by_shape(
    closure: *const ClosureHeader,
) -> Option<Option<f64>> {
    let id = (*closure).shape_id;
    let slot = (id as usize).wrapping_mul(0x9E37_79B9) >> 26 & (VERDICT_CACHE_LEN - 1);
    // SAFETY: this agent's own cell; no reference to it outlives the read.
    let cached = PROTOTYPE_SLOT_CACHE.with(|c| (*c.as_ptr())[slot]);
    let index = if cached.0 == id && id != 0 {
        cached.1
    } else {
        let index = prototype_slot_of_shape(closure, id);
        PROTOTYPE_SLOT_CACHE.with(|c| (*c.as_ptr())[slot] = (id, index));
        index
    };
    match index {
        PROTOTYPE_SLOT_UNKNOWN => None,
        PROTOTYPE_SLOT_ABSENT => Some(None),
        index => {
            let bag = (*closure).props;
            debug_assert!(!bag.is_null(), "a keyed Function shape has a bag");
            let fields = (bag as *const u8).add(std::mem::size_of::<crate::object::ObjectHeader>())
                as *const u64;
            Some(Some(f64::from_bits(*fields.add(index as usize))))
        }
    }
}

const PROTOTYPE_SLOT_UNKNOWN: u32 = u32::MAX;
const PROTOTYPE_SLOT_ABSENT: u32 = u32::MAX - 1;

crate::perry_thread_local! {
    /// Per-agent cache of Function ShapeIds' inline slot of `prototype`
    /// ([`closure_own_prototype_by_shape`]); ShapeIds are never reused, so an
    /// entry can only go unused, never wrong.
    static PROTOTYPE_SLOT_CACHE: std::cell::Cell<[(u32, u32); VERDICT_CACHE_LEN]> =
        const { std::cell::Cell::new([(0, 0); VERDICT_CACHE_LEN]) };
}

/// The inline slot of `prototype` the Function ShapeId `id` describes, or
/// one of the two markers.
#[cold]
#[inline(never)]
unsafe fn prototype_slot_of_shape(closure: *const ClosureHeader, id: u32) -> u32 {
    if id == function_dictionary_shape() || is_class_info((*closure).info) {
        return PROTOTYPE_SLOT_UNKNOWN;
    }
    let Some(descriptor) = shapes::shape_descriptor_by_id(id) else {
        return PROTOTYPE_SLOT_UNKNOWN;
    };
    if descriptor.object_kind != ShapeObjectKind::Function {
        return PROTOTYPE_SLOT_UNKNOWN;
    }
    if descriptor.keys == 0 || descriptor.logical_key_count == 0 {
        return PROTOTYPE_SLOT_ABSENT;
    }
    let keys = descriptor.keys as usize as *const crate::array::ArrayHeader;
    match crate::object::keys_find_slot_by_bytes_resolved(
        keys,
        descriptor.logical_key_count,
        b"prototype",
    ) {
        None => PROTOTYPE_SLOT_ABSENT,
        Some(index)
            if (index as u32) < descriptor.live_inline_slot_count
                && !crate::object::key_attrs::key_is_accessor_at(keys, index as u32) =>
        {
            index as u32
        }
        Some(_) => PROTOTYPE_SLOT_UNKNOWN,
    }
}

/// Raw kind probe for a pointer the caller has already range/band-checked
/// (the successor of the old `*(ptr + 12) == CLOSURE_MAGIC` read, with the
/// same safety contract): the GC header's type byte says CLOSURE and the
/// cell has not been evacuated.
///
/// # Safety
/// `ptr` is a heap address whose preceding 8 bytes are readable.
#[inline(always)]
pub unsafe fn closure_kind_probe(ptr: usize) -> bool {
    let header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    (*header).obj_type == crate::gc::GC_TYPE_CLOSURE
        && (*header).gc_flags & crate::gc::GC_FLAG_FORWARDED == 0
}

/// A property funnel keyed by raw address installed something on `owner`
/// that a base Function shape does not describe (a symbol key, an accessor,
/// a deleted intrinsic, a recorded [[Prototype]], a non-intrinsic string
/// key). If `owner` is a function object, it leaves its base shape.
/// Arbitrary words are fine: ownership is proven before any header byte is
/// trusted (`is_closure_ptr`).
#[inline]
pub(crate) fn note_function_own_state_changed(owner: usize) {
    if super::is_closure_ptr(owner) {
        // SAFETY: `is_closure_ptr` proved a live, non-forwarded closure cell.
        unsafe { closure_become_dictionary(owner as *mut ClosureHeader) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::closure::{
        closure_set_dynamic_prop, closure_set_static_prototype, js_closure_alloc,
    };

    extern "C" fn plain_body(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        1.0
    }
    extern "C" fn async_body(_c: *const ClosureHeader, _this: crate::closure::JsThis) -> f64 {
        2.0
    }

    fn kind_of(c: *const ClosureHeader) -> Option<ShapeObjectKind> {
        shapes::shape_object_kind_by_id(unsafe { (*c).shape_id })
    }

    static PLAIN_BODY: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
        plain_body as crate::codegen_abi::JsBody0<ClosureHeader>,
    );
    static ASYNC_BODY: crate::closure::JsFunctionInfo = crate::closure::JsFunctionInfo::of(
        async_body as crate::codegen_abi::JsBody0<ClosureHeader>,
    )
    .with_flags(crate::closure::FN_ASYNC);

    fn fresh(info: &'static crate::closure::JsFunctionInfo) -> *mut ClosureHeader {
        js_closure_alloc(info, 0)
    }

    #[test]
    fn a_closure_is_born_with_the_base_function_shape_at_plus_four() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let c = fresh(&PLAIN_BODY);
        let word =
            unsafe { *((c as *const u8).add(super::super::CLOSURE_SHAPE_OFFSET) as *const u32) };
        assert!(shapes::is_exotic_shape_id(word), "{word:#x}");
        assert!(
            !shapes::is_site_matchable_shape_id(word),
            "no own-slot site may hold it"
        );
        assert_eq!(kind_of(c), Some(ShapeObjectKind::Function));
        assert_eq!(
            shapes::shape_proto_id(word),
            Some(INTRINSIC_SERIAL_FUNCTION)
        );
        assert!(unsafe { closure_on_base_shape(c) });
        assert!(crate::closure::is_closure_ptr(c as usize));
    }

    #[test]
    fn an_async_body_is_born_with_the_async_function_prototype() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();

        let c = fresh(&ASYNC_BODY);
        let id = unsafe { (*c).shape_id };
        assert_eq!(kind_of(c), Some(ShapeObjectKind::Function));
        assert_eq!(
            shapes::shape_proto_id(id),
            Some(INTRINSIC_SERIAL_ASYNC_FUNCTION)
        );
        assert_ne!(id, unsafe { (*fresh(&PLAIN_BODY)).shape_id });
    }

    /// An own string key moves a function to a KEYED Function shape: the
    /// same key list gives the same ShapeId (canonical per facts), the value
    /// lives in the bag, and the shape stays described (not dictionary).
    #[test]
    fn own_keys_give_a_canonical_keyed_function_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let a = fresh(&PLAIN_BODY);
        let b = fresh(&PLAIN_BODY);
        let base = unsafe { (*a).shape_id };
        for c in [a, b] {
            closure_set_dynamic_prop(c as usize, "tag", 7.0);
            closure_set_dynamic_prop(c as usize, "kind", 8.0);
        }
        let ka = unsafe { (*a).shape_id };
        assert_ne!(ka, base, "an own key leaves the base shape");
        assert_eq!(ka, unsafe { (*b).shape_id }, "same keys, same ShapeId");
        assert_eq!(kind_of(a), Some(ShapeObjectKind::Function));
        assert!(shapes::is_exotic_shape_id(ka) && !shapes::is_site_matchable_shape_id(ka));
        assert_eq!(shapes::shape_proto_id(ka), Some(INTRINSIC_SERIAL_FUNCTION));
        assert!(unsafe { closure_on_base_shape(a) });
        assert!(!function_shape_inherits_from_function_prototype(ka, b"tag"));
        assert!(function_shape_inherits_from_function_prototype(ka, b"bind"));
        assert_eq!(
            crate::closure::closure_get_own_dynamic_prop(a as usize, "kind"),
            Some(8.0)
        );
        // Attributed keys are removed by rebuilding the canonical list, so
        // the function retains the keyed shape described by its own bag.
        assert!(crate::closure::closure_delete_own_dynamic_prop(
            a as usize, "tag"
        ));
        assert_eq!(kind_of(a), Some(ShapeObjectKind::Function));
        let bag = unsafe { (*a).props };
        let bag_desc = unsafe { shapes::object_shape_descriptor(bag) }.unwrap();
        let fn_desc = shapes::shape_descriptor_by_id(unsafe { (*a).shape_id }).unwrap();
        assert_eq!(fn_desc.keys, bag_desc.keys);
        assert_eq!(fn_desc.logical_key_count, bag_desc.logical_key_count);
        assert_eq!(
            crate::closure::closure_get_own_dynamic_prop(a as usize, "tag"),
            None
        );
    }

    #[test]
    fn a_recorded_prototype_a_delete_and_an_accessor_each_leave_the_base_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let a = fresh(&PLAIN_BODY);
        let proto = crate::object::js_object_alloc(0, 0);
        closure_set_static_prototype(
            a as usize,
            crate::value::js_nanbox_pointer(proto as i64).to_bits(),
        );
        assert_eq!(kind_of(a), Some(ShapeObjectKind::FunctionDictionary));

        let b = fresh(&PLAIN_BODY);
        crate::closure::closure_mark_key_deleted(b as usize, "length");
        assert_eq!(kind_of(b), Some(ShapeObjectKind::FunctionDictionary));

        let c = fresh(&PLAIN_BODY);
        crate::object::descriptor_state::set_accessor_descriptor(
            c as usize,
            "x".to_string(),
            crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 },
        );
        assert_eq!(kind_of(c), Some(ShapeObjectKind::FunctionDictionary));
    }

    #[test]
    fn a_symbol_keyed_own_property_leaves_the_base_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let c = fresh(&PLAIN_BODY);
        let sym =
            unsafe { crate::symbol::js_symbol_new(f64::from_bits(crate::value::TAG_UNDEFINED)) };
        crate::symbol::store_object_symbol_property_root(
            c as usize,
            (sym.to_bits() & crate::value::POINTER_MASK) as usize,
            1.0f64.to_bits(),
        );
        assert_eq!(kind_of(c), Some(ShapeObjectKind::FunctionDictionary));
    }

    #[test]
    fn is_closure_ptr_answers_from_the_gc_kind_not_the_shape_word_alone() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let obj = crate::object::js_object_alloc(0, 0);
        let arr = crate::array::js_array_alloc(4);
        assert!(!crate::closure::is_closure_ptr(obj as usize));
        assert!(!crate::closure::is_closure_ptr(arr as usize));
        // Forge a Function ShapeId into an OBJECT's shape word: the header
        // still says OBJECT, so it is not a function.
        unsafe {
            let saved = (*obj).parent_class_id;
            (*obj).parent_class_id = function_base_shape(FunctionProtoKind::Function);
            assert!(!crate::closure::is_closure_ptr(obj as usize));
            assert!(!closure_kind_probe(obj as usize));
            (*obj).parent_class_id = saved;
        }
        assert!(crate::closure::is_closure_ptr(fresh(&PLAIN_BODY) as usize));
    }

    extern "C" fn three_arg_body(
        _c: *const ClosureHeader,
        _this: crate::closure::JsThis,
        a: f64,
        b: f64,
        c: f64,
    ) -> f64 {
        a + b + c
    }

    /// A bind result carries its `.length` in capture 4 (read back through
    /// `builtin_closure_length`, the one reader every `.length` path uses)
    /// and its captures are one tag-checked birth, so a bind leaves no
    /// per-object mask and no metadata-table entry for a minor to prune.
    #[test]
    fn a_bind_result_keeps_its_length_in_a_capture() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();

        let target = js_closure_alloc(crate::fn_info!(three_arg_body, 3; with_declared(3)), 0);
        let args = [f64::from_bits(crate::value::TAG_UNDEFINED), 1.0];
        let bound = unsafe {
            crate::closure::js_function_bind(
                crate::value::js_nanbox_pointer(target as i64),
                args.as_ptr(),
                args.len(),
            )
        };
        let b = (bound.to_bits() & crate::value::POINTER_MASK) as usize;
        assert_eq!(unsafe { crate::closure::bound_function_length(b) }, Some(2));
        assert_eq!(
            crate::object::native_module::builtin_closure_length(b),
            Some(2)
        );
        assert_eq!(
            unsafe { (*(b as *const ClosureHeader)).capture_count },
            5,
            "target, this, partial args, name snapshot, bound length"
        );
        // The bound call still sees target + partial args + call args.
        let r = crate::closure::js_closure_call2(
            b as *const ClosureHeader,
            crate::closure::plain_call_receiver(),
            2.0,
            3.0,
        );
        assert_eq!(r, 6.0);
    }

    /// Every accessor installer leaves the base shape, so the base-shape read
    /// shortcut in `closure_get_dynamic_prop` (which skips the accessor table)
    /// can never skip a real getter.
    #[test]
    fn every_accessor_installer_leaves_the_base_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let acc = crate::object::descriptor_state::AccessorDescriptor { get: 0, set: 0 };
        let attrs = crate::object::PropertyAttrs::new(false, false, true);
        let a = fresh(&PLAIN_BODY);
        crate::object::descriptor_state::install_fresh_accessor_property(
            a as usize,
            "length".into(),
            acc,
            attrs,
        );
        assert_eq!(kind_of(a), Some(ShapeObjectKind::FunctionDictionary));
        let b = fresh(&PLAIN_BODY);
        crate::object::descriptor_state::set_builtin_accessor_descriptor(
            b as usize,
            "name".into(),
            acc,
            attrs,
        );
        assert_eq!(kind_of(b), Some(ShapeObjectKind::FunctionDictionary));
    }
}
