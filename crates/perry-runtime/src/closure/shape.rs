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

    /// The body kind recorded for `func_ptr` — the same registries
    /// `generator_function_proto_of` answers [[GetPrototypeOf]] from.
    pub(crate) fn of_body(func_ptr: *const u8) -> FunctionProtoKind {
        if super::is_registered_async_generator_function(func_ptr) {
            FunctionProtoKind::AsyncGenerator
        } else if super::is_registered_generator_function(func_ptr) {
            FunctionProtoKind::Generator
        } else if super::is_registered_async_function(func_ptr) {
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
    /// One-entry body cache: the last `func_ptr` born and its base shape.
    static LAST_BODY: std::cell::Cell<(usize, u32)> = const { std::cell::Cell::new((0, 0)) };
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
        function_shape_summary(kind),
    ));
    // A keyless intrinsic shape: rooted for the agent's life, so the
    // post-full-trace prune can never retire an id live closures carry.
    // SAFETY: the descriptor was just published on this agent.
    unsafe { shapes::note_external_shape_carrier(shapes::shape_descriptor_by_id(id)) };
    id
}

fn base_slot(index: usize, kind: ShapeObjectKind, proto_id: u64) -> u32 {
    let mut ids = BASE_SHAPES.with(std::cell::Cell::get);
    if ids[index] == 0 {
        ids[index] = mint(kind, proto_id);
        BASE_SHAPES.with(|c| c.set(ids));
    }
    ids[index]
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

/// The ShapeId a fresh closure of `func_ptr` is born with.
#[inline]
pub(crate) fn birth_shape_for_body(func_ptr: *const u8) -> u32 {
    let (last, id) = LAST_BODY.with(std::cell::Cell::get);
    if last == func_ptr as usize && id != 0 {
        return id;
    }
    let id = function_base_shape(FunctionProtoKind::of_body(func_ptr));
    LAST_BODY.with(|c| c.set((func_ptr as usize, id)));
    id
}

/// A body was (re)classified as async/generator: forget the one-entry cache
/// so the next birth re-reads the registries.
#[inline]
pub(crate) fn forget_body_classification(func_ptr: *const u8) {
    LAST_BODY.with(|c| {
        if c.get().0 == func_ptr as usize {
            c.set((0, 0));
        }
    });
}

/// Is this closure still on its base (intrinsic-only) shape?
///
/// # Safety
/// `closure` is a live, non-forwarded `GC_TYPE_CLOSURE` cell.
#[cfg(test)]
#[inline]
pub(crate) unsafe fn closure_has_base_shape(closure: *const ClosureHeader) -> bool {
    shapes::shape_object_kind_by_id((*closure).shape_id) == Some(ShapeObjectKind::Function)
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
    if (*closure).shape_id != dict {
        // GC_STORE_AUDIT(POINTER_FREE): a ShapeId, never a heap reference.
        (*closure).shape_id = dict;
    }
}

/// The intrinsic own properties a base Function shape stands for.
#[inline]
pub(crate) fn is_intrinsic_function_key(key: &str) -> bool {
    is_intrinsic_function_key_bytes(key.as_bytes())
}

/// [`is_intrinsic_function_key`] over raw key bytes.
#[inline]
pub(crate) fn is_intrinsic_function_key_bytes(key: &[u8]) -> bool {
    matches!(key, b"name" | b"length" | b"prototype")
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

    extern "C" fn plain_body(_c: *const ClosureHeader) -> f64 {
        1.0
    }
    extern "C" fn async_body(_c: *const ClosureHeader) -> f64 {
        2.0
    }

    fn kind_of(c: *const ClosureHeader) -> Option<ShapeObjectKind> {
        shapes::shape_object_kind_by_id(unsafe { (*c).shape_id })
    }

    fn fresh(body: extern "C" fn(*const ClosureHeader) -> f64) -> *mut ClosureHeader {
        js_closure_alloc(body as *const u8, 0)
    }

    #[test]
    fn a_closure_is_born_with_the_base_function_shape_at_plus_four() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let c = fresh(plain_body);
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
        assert!(unsafe { closure_has_base_shape(c) });
        assert!(crate::closure::is_closure_ptr(c as usize));
    }

    #[test]
    fn an_async_body_is_born_with_the_async_function_prototype() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        crate::closure::js_register_closure_async_function(async_body as *const u8);
        let c = fresh(async_body);
        let id = unsafe { (*c).shape_id };
        assert_eq!(kind_of(c), Some(ShapeObjectKind::Function));
        assert_eq!(
            shapes::shape_proto_id(id),
            Some(INTRINSIC_SERIAL_ASYNC_FUNCTION)
        );
        assert_ne!(id, unsafe { (*fresh(plain_body)).shape_id });
    }

    #[test]
    fn intrinsic_keys_keep_the_base_shape_and_anything_else_leaves_it() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let c = fresh(plain_body);
        closure_set_dynamic_prop(c as usize, "prototype", 1.0);
        closure_set_dynamic_prop(c as usize, "name", 1.0);
        assert_eq!(kind_of(c), Some(ShapeObjectKind::Function));
        closure_set_dynamic_prop(c as usize, "tag", 7.0);
        assert_eq!(kind_of(c), Some(ShapeObjectKind::FunctionDictionary));
        assert_eq!(unsafe { (*c).shape_id }, function_dictionary_shape());
    }

    #[test]
    fn a_recorded_prototype_a_delete_and_an_accessor_each_leave_the_base_shape() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _t = crate::gc::GcTriggerThresholdTestGuard::suppress_automatic_triggers();
        let a = fresh(plain_body);
        let proto = crate::object::js_object_alloc(0, 0);
        closure_set_static_prototype(
            a as usize,
            crate::value::js_nanbox_pointer(proto as i64).to_bits(),
        );
        assert_eq!(kind_of(a), Some(ShapeObjectKind::FunctionDictionary));

        let b = fresh(plain_body);
        crate::closure::closure_mark_key_deleted(b as usize, "length");
        assert_eq!(kind_of(b), Some(ShapeObjectKind::FunctionDictionary));

        let c = fresh(plain_body);
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
        let c = fresh(plain_body);
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
        assert!(crate::closure::is_closure_ptr(fresh(plain_body) as usize));
    }
}
