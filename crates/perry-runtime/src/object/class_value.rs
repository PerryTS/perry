//! A class CONSTRUCTOR used as a value (#11414; the every-receiver-shape
//! lane's class-constructor stage).
//!
//! Two forms name a class constructor while the migration runs:
//!
//! * the legacy INT32 immediate `0x7FFE_0000_0000_0000 | class_id` with bit 32
//!   clear (bit 32 set is the `C.prototype` half — [`super::class_prototype_ref_id`]).
//!   It is bit-identical to the int32 number equal to the class id, which is
//!   #11414; the lane deletes it;
//! * a class FUNCTION OBJECT: a `GC_TYPE_CLOSURE` cell whose code pointer is
//!   [`js_class_constructor_called`] (its [[Call]], which throws) and whose
//!   capture slot 0 holds the class id as an INT32 value.
//!
//! Every decoder asks [`class_value_id`] (or [`class_value_id_bits`] /
//! [`class_closure_id`] for the raw-word and raw-pointer spellings). Nothing
//! else may test the INT32 tag or the code pointer to decide "is this a class".
use crate::closure::ClosureHeader;

/// The class a constructor VALUE names — either form — or `None`. A
/// `C.prototype` reference is not a constructor and answers `None`.
#[inline]
pub(crate) fn class_value_id(value: f64) -> Option<u32> {
    class_value_id_bits(value.to_bits())
}

/// [`class_value_id`] on the NaN-boxed word.
#[inline]
pub(crate) fn class_value_id_bits(bits: u64) -> Option<u32> {
    match bits >> 48 {
        0x7FFE => {
            if bits & super::native_module::CLASS_PROTOTYPE_REF_FLAG != 0 {
                return None;
            }
            let class_id = (bits & 0xFFFF_FFFF) as u32;
            (class_id != 0 && super::is_class_id_registered(class_id)).then_some(class_id)
        }
        0x7FFD => class_closure_id((bits & crate::value::POINTER_MASK) as usize),
        _ => None,
    }
}

/// The class id of a class function object at raw address `ptr`, or `None`
/// for any other word. Ownership is proven (`is_closure_ptr`) before a header
/// byte is trusted, so arbitrary addresses are fine.
///
/// Callers include hot generic paths (bind, method values), so an ordinary
/// function is rejected before the ownership proof: once the address is a
/// plausible, aligned heap address whose ShapeId word is in the exotic band
/// (the same pre-checks `is_closure_ptr` makes before its first load), the
/// code-pointer word at +8 — inside every exotic cell's header — must be this
/// module's thunk. Only then is the cell proven.
#[inline]
pub fn class_closure_id(ptr: usize) -> Option<u32> {
    if !crate::value::addr_class::is_plausible_heap_addr(ptr)
        || !ptr.is_multiple_of(std::mem::align_of::<ClosureHeader>())
    {
        return None;
    }
    // SAFETY: a plausible, aligned heap address (the contract of the
    // `is_closure_ptr` pre-checks this mirrors).
    let shape =
        unsafe { *((ptr as *const u8).add(crate::closure::CLOSURE_SHAPE_OFFSET) as *const u32) };
    if !crate::object::shapes::is_exotic_shape_id(shape) {
        return None;
    }
    // SAFETY: an exotic-band ShapeId word means a closure-or-exotic header,
    // at least 16 bytes; +8 is the code pointer of a closure.
    let code = unsafe { *((ptr as *const u8).add(8) as *const *const u8) };
    if code != js_class_constructor_called as *const u8 {
        return None;
    }
    class_closure_id_exotic(ptr)
}

/// [`class_closure_id`] past its inline pre-filter (a plausible, aligned heap
/// address whose ShapeId word is in the exotic band): out of line, so the
/// many gates that inline the pre-filter stay small.
#[inline(never)]
fn class_closure_id_exotic(ptr: usize) -> Option<u32> {
    if !crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    // SAFETY: `is_closure_ptr` proved a live, non-forwarded closure cell.
    unsafe { class_closure_id_unchecked(ptr as *const ClosureHeader) }
}

/// [`class_closure_id`] for a cell already proven to be a live closure.
///
/// # Safety
/// `closure` is a live, non-forwarded `GC_TYPE_CLOSURE` cell.
#[inline]
pub(crate) unsafe fn class_closure_id_unchecked(closure: *const ClosureHeader) -> Option<u32> {
    if (*closure).func_ptr != js_class_constructor_called as *const u8 {
        return None;
    }
    let slot0 = *((closure as *const u8).add(std::mem::size_of::<ClosureHeader>()) as *const u64);
    Some((slot0 & 0xFFFF_FFFF) as u32)
}

/// [[Call]] of a class constructor: ES2015 9.2.1 step 2 — a class constructor
/// called without `new` throws a TypeError. It is the code pointer of every
/// class function object (and how one is recognized); [[Construct]] never
/// reaches it — `new` decodes the class id and runs the class's constructor.
#[no_mangle]
pub unsafe extern "C" fn js_class_constructor_called(closure: *const ClosureHeader) -> f64 {
    let name = unsafe { class_closure_id_unchecked(closure) }
        .and_then(super::class_registry::class_name_for_id)
        .unwrap_or_default();
    let message = format!("Class constructor {name} cannot be invoked without 'new'");
    crate::node_submodules::diagnostics::throw_type_error_no_code(message.as_bytes())
}

/// The PRE-MIGRATION gate spelling, kept exact for the legacy form while it
/// also admits the function-object form: any INT32 word (registered or not,
/// either half — those gates accepted every `0x7FFE` word, which is #11414)
/// or a class function object. `bits` is a NaN-boxed VALUE word. Every caller
/// is narrowed to [`class_value_id_bits`] when the INT32 form is deleted.
#[inline]
pub(crate) fn legacy_class_value_word(bits: u64) -> Option<u32> {
    match bits >> 48 {
        0x7FFE => Some((bits & 0xFFFF_FFFF) as u32),
        0x7FFD => class_closure_id((bits & crate::value::POINTER_MASK) as usize),
        _ => None,
    }
}

/// [`legacy_class_value_word`] for a word that arrived through a POINTER-typed
/// parameter (`obj as u64`), which may be a raw untagged heap address.
#[inline]
pub(crate) fn legacy_class_ptr_word(bits: u64) -> Option<u32> {
    match bits >> 48 {
        0 => class_closure_id(bits as usize),
        _ => legacy_class_value_word(bits),
    }
}

/// The NaN-boxed VALUE for a word [`legacy_class_ptr_word`] admitted: a raw
/// heap address gets its POINTER tag, anything else is already a value.
#[inline]
pub(crate) fn boxed_class_word(bits: u64) -> f64 {
    if bits >> 48 == 0 {
        f64::from_bits(crate::value::POINTER_TAG | bits)
    } else {
        f64::from_bits(bits)
    }
}

// ---------------------------------------------------------------------------
// The function object for each class: one per agent per class id.
// ---------------------------------------------------------------------------

/// Class ids per table page (log2). Class ids are dense per program plus a few
/// high reserved ids for built-in classes, so a two-level table keeps the
/// lookup a pair of indexed loads without a large flat array.
const CLASS_VALUE_PAGE_SHIFT: u32 = 8;
const CLASS_VALUE_PAGE_LEN: usize = 1 << CLASS_VALUE_PAGE_SHIFT;

type ClassValuePage = [*mut ClosureHeader; CLASS_VALUE_PAGE_LEN];

crate::perry_thread_local! {
    /// This agent's class function objects, indexed by class id: a page
    /// directory (`pages`, `len` pages) whose pages are leaked for the agent's
    /// life. Read without a borrow flag — the hot path is a TLS read, a bounds
    /// check and two loads. A GC root (rewritten on a move) via
    /// [`scan_class_value_roots_mut`].
    static CLASS_VALUES: std::cell::Cell<(*mut *mut ClassValuePage, usize)> =
        const { std::cell::Cell::new((std::ptr::null_mut(), 0)) };
}

#[inline]
fn class_value_cached(class_id: u32) -> Option<*mut ClosureHeader> {
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = class_id as usize & (CLASS_VALUE_PAGE_LEN - 1);
    let (pages, len) = CLASS_VALUES.with(std::cell::Cell::get);
    if page >= len {
        return None;
    }
    // SAFETY: `pages` holds `len` page pointers (null or a live leaked page).
    unsafe {
        let p = *pages.add(page);
        if p.is_null() {
            return None;
        }
        let c = (*p)[index];
        (!c.is_null()).then_some(c)
    }
}

/// The table slot for `class_id`, growing the directory / minting the page.
fn class_value_slot(class_id: u32) -> *mut *mut ClosureHeader {
    let page = (class_id >> CLASS_VALUE_PAGE_SHIFT) as usize;
    let index = class_id as usize & (CLASS_VALUE_PAGE_LEN - 1);
    let (mut pages, mut len) = CLASS_VALUES.with(std::cell::Cell::get);
    if page >= len {
        let new_len = (page + 1).next_power_of_two().max(4);
        let mut dir: Vec<*mut ClassValuePage> = vec![std::ptr::null_mut(); new_len];
        if !pages.is_null() {
            // SAFETY: the old directory holds `len` entries.
            unsafe { dir[..len].copy_from_slice(std::slice::from_raw_parts(pages, len)) };
            // The old directory is leaked: a concurrent reader on this agent
            // cannot exist (single-threaded agent), but the few bytes are not
            // worth a free/reuse protocol.
        }
        pages = Box::leak(dir.into_boxed_slice()).as_mut_ptr();
        len = new_len;
        CLASS_VALUES.with(|c| c.set((pages, len)));
    }
    // SAFETY: `page < len`.
    unsafe {
        let slot = pages.add(page);
        if (*slot).is_null() {
            *slot = Box::leak(Box::new([std::ptr::null_mut(); CLASS_VALUE_PAGE_LEN]));
        }
        (**slot).as_mut_ptr().add(index)
    }
}

/// Allocate the class function object for `class_id`: a closure born in the
/// old generation and pinned (it lives as long as the agent and never moves),
/// code pointer
/// [`js_class_constructor_called`], capture slot 0 = the class id as INT32.
///
/// Never collects: callers hold raw receiver pointers across the lookup, so
/// the old-arena allocation runs under a [`crate::gc::GcSuppressScope`].
#[cold]
#[inline(never)]
fn class_value_mint(class_id: u32) -> *mut ClosureHeader {
    let _no_collect = crate::gc::GcSuppressScope::new();
    let payload = crate::closure::closure_payload_size(1);
    let ptr = crate::arena::arena_alloc_gc_old_born_tenured(
        payload,
        std::mem::align_of::<ClosureHeader>(),
        crate::gc::GC_TYPE_CLOSURE,
    ) as *mut ClosureHeader;
    unsafe {
        // GC_STORE_AUDIT(INIT): fresh class function object; the one capture
        // is an INT32 class id and the props edge is null — pointer-free.
        (*ptr).capture_count = 1;
        (*ptr).shape_id = crate::closure::shape::function_class_shape();
        (*ptr).func_ptr = js_class_constructor_called as *const u8;
        (*ptr).props = std::ptr::null_mut();
        std::ptr::write(
            crate::closure::closure_capture_slots_mut(ptr),
            crate::value::INT32_TAG | class_id as u64,
        );
        crate::gc::layout_init_pointer_free(ptr as *mut u8);
        // Born old AND pinned: the address is the class's identity for the
        // agent's life (compiled code keeps it in registers and allocas, the
        // metadata and weak tables compare it), so no collector may move it.
        crate::gc::pin_user_ptr_non_young(ptr as *mut u8);
    }
    // SAFETY: the slot is this agent's table entry for `class_id`.
    unsafe { *class_value_slot(class_id) = ptr };
    crate::gc::runtime_write_barrier_root_heap_word(ptr as u64);
    // Still inside the no-collect scope: the own-property object and its
    // keys allocate.
    for key in INTRINSIC_OWN_DATA_KEYS {
        install_intrinsic_own_data(class_id, key);
    }
    ptr
}

/// A class constructor's `length` and `name`, in creation order
/// (ClassDefinitionEvaluation: SetFunctionLength, then SetFunctionName).
const INTRINSIC_OWN_DATA_KEYS: [&str; 2] = ["length", "name"];

/// The attributes of a function's own `length` / `name`.
const INTRINSIC_ATTRS: (bool, bool, bool) = (false, false, true);

/// The value of intrinsic own data property `key` of class `class_id`, if
/// the class registered one.
fn intrinsic_own_data_value(class_id: u32, key: &str) -> Option<f64> {
    match key {
        "length" => super::class_registry::class_length_for_id(class_id).map(f64::from),
        "name" => super::class_registry::class_name_for_id(class_id).map(|name| {
            let s = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
            f64::from_bits(crate::value::JSValue::string_ptr(s).bits())
        }),
        _ => None,
    }
}

/// Does a static method or accessor of class `class_id` own `key`? Then it,
/// not the intrinsic data property, is the class's own `key`.
fn static_member_owns(class_id: u32, key: &str) -> bool {
    super::class_registry::class_has_own_static_method(class_id, key)
        || super::class_registry::class_own_static_accessor_ptrs(class_id, key).is_some()
}

/// Is own `key` of class `class_id` still the intrinsic data property (not
/// replaced by a static field, a `defineProperty`, or deleted)?
fn holds_intrinsic(class_id: u32, key: &str) -> bool {
    class_static_get(class_id, key).is_some()
        && super::class_registry::class_static_defined_attrs(class_id, key) == Some(INTRINSIC_ATTRS)
}

/// ClassDefinitionEvaluation's SetFunctionLength / SetFunctionName: `length`
/// and `name` are own DATA properties of the class's function object,
/// `{ writable: false, enumerable: false, configurable: true }`, kept in its
/// own-property object with every other own data property — so `C.name` and
/// `x.constructor.name` are one lookup in that object's shape. A static
/// method or accessor of the same name is the class's own property instead,
/// and a static field or `defineProperty` of that name replaces it.
fn install_intrinsic_own_data(class_id: u32, key: &str) {
    if static_member_owns(class_id, key) {
        return;
    }
    let Some(value) = intrinsic_own_data_value(class_id, key) else {
        return;
    };
    class_static_set(class_id, key, value);
    let (writable, enumerable, configurable) = INTRINSIC_ATTRS;
    super::class_registry::class_static_set_defined_attrs(
        class_id,
        key,
        writable,
        enumerable,
        configurable,
    );
}

/// A static field `key` was defined on class `class_id`: if it replaced the
/// intrinsic `name` / `length`, the property keeps the field's (ordinary)
/// attributes, not the intrinsic's.
pub(crate) fn note_static_field_defined(class_id: u32, key: &str) {
    if INTRINSIC_OWN_DATA_KEYS.contains(&key)
        && super::class_registry::class_static_defined_attrs(class_id, key) == Some(INTRINSIC_ATTRS)
    {
        super::class_registry::class_static_clear_defined_attrs(class_id, key);
    }
}

/// The registry changed what class `class_id`'s intrinsic `key` is (its
/// name or length registered, or a static method / accessor of that name
/// registered) after this agent minted its function object: bring the own
/// property in line. A key the program already redefined or deleted is left
/// alone.
pub(crate) fn note_intrinsic_registration(class_id: u32, key: &str) {
    if !INTRINSIC_OWN_DATA_KEYS.contains(&key) || class_value_cached(class_id).is_none() {
        return;
    }
    let _no_collect = crate::gc::GcSuppressScope::new();
    if holds_intrinsic(class_id, key) {
        if static_member_owns(class_id, key) {
            class_static_remove(class_id, key);
            super::class_registry::class_static_clear_defined_attrs(class_id, key);
        } else if let Some(value) = intrinsic_own_data_value(class_id, key) {
            class_static_set(class_id, key, value);
        }
    } else if class_static_get(class_id, key).is_none()
        && !super::class_registry::class_is_key_deleted(class_id, key)
    {
        install_intrinsic_own_data(class_id, key);
    }
}

/// The class function object for `class_id` on this agent (minted on first
/// use). `class_id` must be a registered class.
#[inline]
pub(crate) fn class_value_ptr(class_id: u32) -> *mut ClosureHeader {
    match class_value_cached(class_id) {
        Some(c) => c,
        None => class_value_mint(class_id),
    }
}

/// The VALUE of class `class_id`'s constructor: its function object, NaN-boxed.
#[inline]
pub(crate) fn class_value(class_id: u32) -> f64 {
    f64::from_bits(crate::value::POINTER_TAG | (class_value_ptr(class_id) as u64))
}

/// Emitted for every `Expr::ClassRef` and every place compiled code names a
/// class as a value (static `this`, `new.target`, `ns.C`): the class's
/// function object. A per-agent indexed load; never allocates after the
/// first use and never collects.
#[no_mangle]
pub extern "C" fn js_class_value(class_id: i32) -> f64 {
    class_value(class_id as u32)
}

/// GC root scan for [`CLASS_VALUES`]; registered in `gc::mod`'s runtime
/// scanner list.
///
/// A class function object is PINNED, and marking never queues a pinned
/// header (`try_mark_*`: "pinned objects are always live"), so no collector
/// enumerates its child slots from a root. Its one heap edge, the own-property
/// bag (`props`, the statics), is therefore visited here as a root slot of its
/// own: a full trace marks and traces the bag (and notes the shape it carries,
/// which post-trace descriptor retirement reads), and a moving collection
/// rewrites the edge. A minor also reaches the edge through the remembered set
/// the `bag_ensure` store barrier dirtied; the second visit of a rewritten
/// slot sees the forwarded address and is a no-op.
pub(crate) fn scan_class_value_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let (pages, len) = CLASS_VALUES.with(std::cell::Cell::get);
    for i in 0..len {
        // SAFETY: `pages` holds `len` page pointers (null or a live page).
        let page = unsafe { *pages.add(i) };
        if page.is_null() {
            continue;
        }
        // SAFETY: a live leaked page of this agent.
        for slot in unsafe { (*page).iter_mut() } {
            if slot.is_null() {
                continue;
            }
            visitor.visit_raw_mut_ptr_slot(slot);
            // SAFETY: a live class function object of this agent.
            let props = unsafe { &mut (**slot).props };
            if !props.is_null() {
                visitor.visit_raw_mut_ptr_slot(props);
            }
        }
    }
}

/// `C[prop]` for a class function object at `ptr` (routed by
/// `closure_get_dynamic_prop` on the class ShapeId): the class lookup.
#[cold]
#[inline(never)]
pub(crate) fn class_static_read(ptr: usize, prop: &str, key: *const crate::StringHeader) -> f64 {
    // SAFETY: the caller proved a live class closure (its ShapeId).
    let Some(class_id) = (unsafe { class_closure_id_unchecked(ptr as *const ClosureHeader) })
    else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    // The class object is pinned: `ptr` survives the key allocation.
    let key = if key.is_null() {
        crate::string::js_string_from_bytes(prop.as_ptr(), prop.len() as u32)
    } else {
        key as *mut crate::StringHeader
    };
    let value = crate::object::field_get_set::class_value_get_field(
        ptr as *const crate::object::ObjectHeader,
        key,
        ptr as u64,
        class_id,
    );
    f64::from_bits(value.bits())
}

/// A statically lowered `C.x` whose compiled alias is detached (`TAG_HOLE`:
/// the static was deleted, redefined as an accessor or made read-only): the
/// generic [[Get]] on the class function object.
///
/// # Safety
/// `name_ptr` points at `name_len` bytes of UTF-8 (codegen rodata).
#[no_mangle]
pub unsafe extern "C" fn js_class_static_field_get(
    class_id: i32,
    name_ptr: *const u8,
    name_len: i64,
) -> f64 {
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    let receiver = class_value_ptr(class_id as u32) as *const crate::object::ObjectHeader;
    f64::from_bits(crate::object::js_object_get_field_by_name(receiver, key).bits())
}

/// A statically lowered `C.x = v` whose compiled alias is detached: the
/// generic [[Set]] on the class function object (a setter, a read-only
/// refusal, or re-creating a deleted static — which re-attaches the alias).
///
/// # Safety
/// As [`js_class_static_field_get`].
#[no_mangle]
pub unsafe extern "C" fn js_class_static_field_put(
    class_id: i32,
    name_ptr: *const u8,
    name_len: i64,
    value: f64,
) {
    let key = crate::string::js_string_from_bytes(name_ptr, name_len as u32);
    let receiver = class_value_ptr(class_id as u32) as *mut crate::object::ObjectHeader;
    crate::object::js_object_set_field_by_name(receiver, key, value);
}

/// [[Get]] of `key` on class `class_id`'s [[Prototype]], `receiver` as the
/// receiver: the continuation of a read of a key the class does not own
/// (e.g. its own `name` was deleted — `Sub.name` then reads `Base.name`, a
/// base class reads `Function.prototype.name`). The [[Prototype]] is the
/// recorded one (`Object.setPrototypeOf(C, p)`), else the parent class's
/// function object, else the parent function (`extends <function>`), else
/// %Function.prototype%.
pub(crate) fn class_prototype_get(
    class_id: u32,
    key: *const crate::StringHeader,
    receiver: f64,
) -> crate::value::JSValue {
    use crate::value::JSValue;
    if super::class_registry::class_static_prototype_is_nulled(class_id) {
        return JSValue::undefined();
    }
    let proto = super::class_registry::class_static_prototype(class_id) as usize;
    let proto = if proto != 0 {
        proto
    } else if let Some(parent) = super::get_parent_class_id(class_id)
        .filter(|&p| p != 0 && p != class_id && super::is_class_id_registered(p))
    {
        class_value_ptr(parent) as usize
    } else if let Some(parent) = super::class_registry::class_parent_closure(class_id) {
        parent
    } else {
        crate::closure::shape::FUNCTION_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire)
            as usize
    };
    if proto == 0 {
        return JSValue::undefined();
    }
    let prev = super::field_get_set::accessor_receiver_override_begin(receiver);
    let value = super::js_object_get_field_by_name(proto as *const super::ObjectHeader, key);
    super::field_get_set::accessor_receiver_override_end(prev);
    value
}

// ---------------------------------------------------------------------------
// Statics: the class function object's OWN properties.
// ---------------------------------------------------------------------------

/// A runtime-internal static key (private statics, computed-key records,
/// class captures): stored in the function object's internal state record,
/// never as a property.
#[inline]
fn is_internal_static_key(name: &str) -> bool {
    crate::object::is_internal_runtime_key(name)
}

/// Class `class_id`'s own static data property `name` (a declared static
/// field or a runtime `C.x = v`): a slot of its function object's own-property
/// bag.
pub(crate) fn class_static_get(class_id: u32, name: &str) -> Option<f64> {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: `class_value_ptr` returns this agent's live class closure.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_get(ptr, name)
        } else {
            crate::closure::props::bag_get(ptr, name.as_bytes())
        }
    }
}

/// Define/overwrite class `class_id`'s own static data property `name`.
pub(crate) fn class_static_set(class_id: u32, name: &str, value: f64) {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above; the bag writers run under a GcSuppressScope.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_set(ptr, name, value);
        } else {
            crate::closure::props::bag_set(ptr, name, value);
        }
    }
}

/// Remove class `class_id`'s own static data property `name`; true when it
/// existed.
pub(crate) fn class_static_remove(class_id: u32, name: &str) -> bool {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above.
    unsafe {
        if is_internal_static_key(name) {
            crate::closure::props::state_internal_remove(ptr, name)
        } else {
            crate::closure::props::bag_remove(ptr, name)
        }
    }
}

/// Class `class_id`'s own static data properties in own-key order (integer
/// keys ascending, then creation order). Internal keys are not properties and
/// never appear.
pub(crate) fn class_static_entries(class_id: u32) -> Vec<(String, f64)> {
    let ptr = class_value_ptr(class_id) as usize;
    // SAFETY: as above.
    unsafe { crate::closure::props::bag_snapshot(ptr) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn register(cid: u32) {
        let mut guard = crate::object::REGISTERED_CLASS_IDS.write().unwrap();
        guard
            .get_or_insert_with(crate::fast_hash::new_ptr_hash_set)
            .insert(cid);
    }

    /// #11414: a class value is ONE function object per class — never an
    /// INT32 word a number can equal — pinned and old, so its address is its
    /// identity across collections.
    #[test]
    fn class_value_is_one_pinned_function_object_per_class() {
        let cid = 0x6A01;
        register(cid);
        let a = class_value(cid);
        let b = class_value(cid);
        assert_eq!(a.to_bits(), b.to_bits(), "one function object per class");
        let v = crate::value::JSValue::from_bits(a.to_bits());
        assert!(
            !v.is_int32() && !v.is_number(),
            "a class value is not a number"
        );
        assert!(v.is_pointer());
        let ptr = (a.to_bits() & crate::value::POINTER_MASK) as usize;
        assert!(
            crate::closure::is_closure_ptr(ptr),
            "a GC_TYPE_CLOSURE cell"
        );
        assert_eq!(class_value_id(a), Some(cid));
        assert_eq!(class_closure_id(ptr), Some(cid));
        // The number equal to the class id is not the class.
        assert_ne!(
            f64::from_bits(crate::value::INT32_TAG | cid as u64).to_bits(),
            a.to_bits()
        );
        let header = unsafe { crate::value::addr_class::try_read_gc_header(ptr) }.expect("header");
        assert_ne!(header.gc_flags & crate::gc::GC_FLAG_PINNED, 0, "pinned");
        assert_ne!(header.gc_flags & crate::gc::GC_FLAG_TENURED, 0, "born old");
        crate::gc::js_gc_collect();
        assert_eq!(class_value(cid).to_bits(), a.to_bits(), "never moves");
        assert_eq!(class_value_id(a), Some(cid), "survives a full collection");
        let other = class_value(0x6A02);
        assert_ne!(other.to_bits(), a.to_bits());
        assert_eq!(class_value_id(other), Some(0x6A02));
    }

    /// The table is a root: the scan visits every minted class value.
    #[test]
    fn class_value_table_is_scanned() {
        let cid = 0x6B01;
        register(cid);
        let ptr = class_value_ptr(cid) as usize;
        let mut seen = false;
        scan_class_value_roots_mut(&mut crate::gc::RuntimeRootVisitor::for_copy(
            &mut |v: f64| {
                let bits = v.to_bits();
                if bits as usize == ptr || (bits & crate::value::POINTER_MASK) as usize == ptr {
                    seen = true;
                }
            },
        ));
        assert!(seen, "the class-value table must be a GC root");
    }

    /// #11609: the class function object is pinned, and marking never queues a
    /// pinned header, so its own-property bag (the statics) is reached only
    /// because the class-value root scan visits the `props` edge itself.
    /// Without that, a full trace never visits the bag: the shape the bag
    /// carries is never noted as carried, post-trace descriptor retirement
    /// drops it, and every static reads back as absent.
    #[test]
    fn a_full_collection_keeps_the_class_statics_bag() {
        let cid = 0x6B02;
        register(cid);
        // A unit-test thread may not have run `gc_init`'s scanner list.
        crate::gc::gc_register_mutable_root_scanner(scan_class_value_roots_mut);
        let ptr = class_value_ptr(cid) as usize;
        let text = "static-payload-11609";
        let s = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        class_static_set(
            cid,
            "k11609",
            f64::from_bits(crate::value::JSValue::string_ptr(s).bits()),
        );
        let mut saw_bag = false;
        let bag = unsafe { crate::closure::props::bag_of(ptr) } as usize;
        assert_ne!(bag, 0, "the static installed a bag");
        scan_class_value_roots_mut(&mut crate::gc::RuntimeRootVisitor::for_copy(
            &mut |v: f64| {
                let bits = v.to_bits();
                if bits as usize == bag || (bits & crate::value::POINTER_MASK) as usize == bag {
                    saw_bag = true;
                }
            },
        ));
        assert!(
            saw_bag,
            "the root scan must visit the pinned class's bag edge"
        );
        crate::gc::js_gc_collect();
        crate::gc::js_gc_collect();
        let got = class_static_get(cid, "k11609").expect("the static survives a full collection");
        let got = crate::value::JSValue::from_bits(got.to_bits());
        let hdr = got.as_string_ptr();
        assert!(!hdr.is_null());
        let bytes = unsafe { crate::string::OwnedStringBytes::copy_from_header(hdr) };
        assert_eq!(bytes.as_bytes(), text.as_bytes());
        let keys: Vec<String> = class_static_entries(cid)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(keys.iter().any(|k| k == "k11609"), "own keys: {keys:?}");
    }

    /// The kind is a shape fact: class function objects carry their own
    /// ShapeId, distinct from FunctionDictionary (shapes are canonical per
    /// facts — without its marker fact the class shape WOULD be the dictionary
    /// id and every dictionary function would route as a class), and it is
    /// sticky across own-property installs.
    #[test]
    fn class_function_objects_have_their_own_sticky_shape() {
        let cid = 0x6C01;
        register(cid);
        let class_shape = crate::closure::shape::function_class_shape();
        assert_ne!(
            class_shape,
            crate::closure::shape::function_dictionary_shape()
        );
        let ptr = class_value_ptr(cid);
        assert_eq!(unsafe { (*ptr).shape_id }, class_shape);
        class_static_set(cid, "s", 1.0);
        crate::closure::shape::note_function_own_state_changed(ptr as usize);
        assert_eq!(unsafe { (*ptr).shape_id }, class_shape, "sticky");
        extern "C" fn body() {}
        let f = crate::closure::js_closure_alloc(body as *const u8, 0);
        crate::closure::shape::note_function_own_state_changed(f as usize);
        assert_ne!(
            unsafe { (*f).shape_id },
            class_shape,
            "a dictionary function is not a class"
        );
    }

    /// A class constructor's `length` and `name` are own data properties of
    /// its function object (in its own-property object, intrinsic
    /// attributes); a static method of that name owns the key instead.
    #[test]
    fn name_and_length_are_own_data_of_the_function_object() {
        let cid = 0x6D01;
        register(cid);
        unsafe { crate::object::js_register_class_name(cid, b"Zed".as_ptr(), 3) };
        crate::object::js_register_class_length(cid, 2);
        let ptr = class_value_ptr(cid) as usize;
        assert_eq!(
            unsafe { crate::closure::props::bag_get(ptr, b"length") },
            Some(2.0),
            "own length"
        );
        let name = unsafe { crate::closure::props::bag_get(ptr, b"name") }.expect("own name");
        let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
        // SAFETY: a live string value just read from the object.
        let bytes = unsafe {
            crate::string::js_string_key_bytes(
                crate::value::JSValue::from_bits(name.to_bits()),
                &mut scratch,
            )
        }
        .expect("a string");
        assert_eq!(bytes, b"Zed");
        for key in ["length", "name"] {
            assert_eq!(
                crate::object::class_registry::class_static_defined_attrs(cid, key),
                Some(INTRINSIC_ATTRS),
                "{key}: non-writable, non-enumerable, configurable"
            );
        }
        extern "C" fn static_name() -> f64 {
            0.0
        }
        unsafe {
            crate::object::class_registry::js_register_class_static_method(
                cid as i64,
                b"name".as_ptr(),
                4,
                static_name as *const () as usize as i64,
                0,
                0,
            )
        };
        assert_eq!(
            unsafe { crate::closure::props::bag_get(ptr, b"name") },
            None,
            "a static method named `name` is the class's own `name`"
        );
    }

    /// Only the function object's own code pointer names a class.
    #[test]
    fn ordinary_closures_and_numbers_are_not_class_values() {
        extern "C" fn body() {}
        let c = crate::closure::js_closure_alloc(body as *const u8, 0);
        assert_eq!(class_closure_id(c as usize), None);
        assert_eq!(class_value_id(42.0), None);
        assert_eq!(
            class_value_id(f64::from_bits(crate::value::TAG_UNDEFINED)),
            None
        );
    }
}
