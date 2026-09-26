//! The JS-visible `Timeout` / `Immediate` handle object (#340/#341).
//!
//! Split out of `timer.rs` to keep that file under the 2000-line size gate
//! (`scripts/check_file_size.sh`). The boundary is the whole of the handle's
//! object model and nothing else: its two class ids, the packed
//! `ObjectMeta.native_state` word, the per-realm prototype singletons and
//! their GC root scan, the prototype/constructor installers, the receiver
//! plumbing every prototype method shares, and the allocator `timer_object`.
//!
//! What stays in `timer.rs` is the scheduler: the queues, the tick loops, the
//! `js_set_*` / `clear*` entry points and the mock-timer surface. The seam
//! between the two is deliberately narrow -- the scheduler calls
//! `timer_object(id, kind)` to wrap an id it has just minted, and reads an id
//! back out with `timer_handle_id` / `timer_handle_parts`.

use super::*;

// ===========================================================================
// Honest tags (#340/#341): a `Timeout` / `Immediate` handed to JS is an
// ORDINARY object.
//
// `setTimeout` used to return its registry id NaN-boxed with `POINTER_TAG`,
// so a timer was a small integer pretending to be a pointer. That id is not
// even unambiguous: `primitive_methods.rs` had to document that "timer ids and
// perry-ffi registry handles share the pointer-tagged small-integer band and
// both count from 1", so a live HTTP/2 server handle 1 and a `setTimeout` id 1
// were the same value and the method dispatch had to guess between them.
//
// The value JS receives is now a `GC_TYPE_OBJECT` with a family class id, a
// real ShapeId and a per-family prototype carrying `ref` / `unref` / `hasRef` /
// `refresh` / `close`, `Symbol.dispose` and (Timeout only, matching node)
// `Symbol.toPrimitive`. The registry id rides in `ObjectMeta.native_state`, so
// the id stays the runtime's internal currency and only the JS-visible handle
// changes. Seven dispatch arms keyed on `is_known_timer_id` go away with it.
//
// State word: bit 0 present, bit 1 immediate, bits 8.. the timer id.
// ===========================================================================

/// Class ids in the web-builtin block. `0x2401..=0x2406` are
/// AbortController/AbortSignal/Event/CustomEvent/DOMException/EventTarget and
/// `0x2407/8` are TextEncoder/TextDecoder.
pub(crate) const TIMEOUT_CLASS_ID: u32 = crate::native_class_ids::TIMEOUT;
pub(crate) const IMMEDIATE_CLASS_ID: u32 = crate::native_class_ids::IMMEDIATE;

const TIMER_STATE_PRESENT: u64 = 1;
const TIMER_STATE_IMMEDIATE: u64 = 1 << 1;
const TIMER_STATE_ID_SHIFT: u32 = 8;

crate::perry_thread_local! {
    static TIMEOUT_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
    static IMMEDIATE_PROTOTYPE_SLOT: AtomicI64 = const { AtomicI64::new(0) };
}

/// The two prototype singletons, one per realm. Every timer handle's
/// `[[Prototype]]` points at one of them, so they must outlive every timer —
/// the same rooting contract as the `%IteratorPrototype%` tower, and scanned
/// from the same place (`object::scan_object_cache_roots_mut`).
pub(crate) static TIMEOUT_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&TIMEOUT_PROTOTYPE_SLOT);
pub(crate) static IMMEDIATE_PROTOTYPE_PTR: crate::object::RealmAtomicI64 =
    crate::object::RealmAtomicI64::new(&IMMEDIATE_PROTOTYPE_SLOT);

/// GC roots for the prototype singletons. Called from
/// `object::scan_object_cache_roots_mut`, beside the iterator tower.
pub(crate) fn scan_timer_prototype_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    for slot in [&TIMEOUT_PROTOTYPE_PTR, &IMMEDIATE_PROTOTYPE_PTR] {
        slot.with_slot(|slot| {
            visitor.visit_atomic_i64_slot(slot, Ordering::Acquire, Ordering::Release);
        });
    }
}

fn timer_state_word(id: i64, kind: CallbackTimerKind) -> u64 {
    let mut word = TIMER_STATE_PRESENT | ((id as u64) << TIMER_STATE_ID_SHIFT);
    if matches!(kind, CallbackTimerKind::Immediate) {
        word |= TIMER_STATE_IMMEDIATE;
    }
    word
}

/// `(id, is_immediate)` for a timer handle, or `None` for anything else.
/// Gated on the class id in the object header, so a foreign receiver
/// (`Timeout.prototype.ref.call({})`) is refused rather than misread.
pub(crate) fn timer_handle_parts(value: f64) -> Option<(i64, bool)> {
    let bits = value.to_bits();
    if (bits & crate::value::TAG_MASK) != crate::value::POINTER_TAG {
        return None;
    }
    let addr = (bits & crate::value::POINTER_MASK) as usize;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(addr)? };
    if header.obj_type != crate::gc::GC_TYPE_OBJECT {
        return None;
    }
    let obj = addr as *mut crate::object::ObjectHeader;
    unsafe {
        let class_id = (*obj).class_id;
        if class_id != TIMEOUT_CLASS_ID && class_id != IMMEDIATE_CLASS_ID {
            return None;
        }
        let meta = (*obj).meta;
        if meta.is_null() {
            return None;
        }
        let word = (*meta).native_state;
        if word & TIMER_STATE_PRESENT == 0 {
            return None;
        }
        Some((
            (word >> TIMER_STATE_ID_SHIFT) as i64,
            word & TIMER_STATE_IMMEDIATE != 0,
        ))
    }
}

/// The timer id behind a JS value, for `clearTimeout` and friends.
pub(super) fn timer_handle_id(value: f64) -> Option<i64> {
    timer_handle_parts(value).map(|(id, _)| id)
}

fn throw_timer_type_error(message: &[u8]) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    let bits = crate::value::JSValue::pointer(err as *const u8).bits();
    crate::exception::js_throw(f64::from_bits(bits))
}

/// The receiver of a prototype method and, when it is one of ours, its id.
///
/// Measured against node 26.8.1 rather than assumed, because these methods are
/// NOT WebIDL and node does not brand-check them:
///
/// ```text
/// Timeout.prototype.ref.call({})      -> the receiver  (no throw)
/// Timeout.prototype.unref.call({})    -> the receiver
/// Timeout.prototype.refresh.call({})  -> the receiver
/// Timeout.prototype.close.call({})    -> the receiver
/// Timeout.prototype.hasRef.call({})   -> undefined     (not `false`)
/// t[Symbol.toPrimitive].call({})      -> undefined
/// t[Symbol.dispose].call({})          -> undefined
/// ```
///
/// So a foreign receiver is answered, not refused — the opposite of the text
/// family, whose WebIDL accessors throw. Each thunk below returns node's answer
/// for `None` and never touches timer state in that case.
fn timer_receiver() -> (f64, Option<i64>) {
    let this = crate::object::js_implicit_this_get();
    let id = timer_handle_id(this);
    (this, id)
}

extern "C" fn timer_proto_ref_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let (this, id) = timer_receiver();
    if let Some(id) = id {
        js_timer_ref(id);
    }
    this
}

extern "C" fn timer_proto_unref_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let (this, id) = timer_receiver();
    if let Some(id) = id {
        js_timer_unref(id);
    }
    this
}

extern "C" fn timer_proto_has_ref_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let (_, id) = timer_receiver();
    match id {
        Some(id) if js_timer_has_ref(id) != 0 => {
            f64::from_bits(crate::value::JSValue::bool(true).bits())
        }
        Some(_) => f64::from_bits(crate::value::JSValue::bool(false).bits()),
        // node answers `undefined`, not `false`, for a foreign receiver.
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

extern "C" fn timer_proto_refresh_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let (this, id) = timer_receiver();
    if let Some(id) = id {
        js_timer_refresh(id);
    }
    this
}

/// #10522: `t.ref()` / `t.unref()` / `t.hasRef()` / `t.refresh()` answered
/// without the generic dispatch tower, when the call provably resolves to the
/// native method this family installed.
///
/// A timer handle carries a `meta` record (its state word) and a recorded
/// prototype, so the class-vtable fast path (#7769) refuses it, and every call
/// walked the whole tower instead: primitive and handle probes, a by-name
/// prototype read that allocates the key string, then a closure rebind and a
/// call through the thunk. That walk was ~6.3 k instructions of a ~13.7 k
/// `setTimeout` + `unref` + `clearTimeout` round (rate-limiter-flexible's
/// `MemoryStorage` shape), more than the timer bookkeeping itself.
///
/// The fast path resolves the property itself, with no cache to go stale:
///
/// * the receiver is a timer handle whose `[[Prototype]]` is still its
///   family's prototype, with no own string keys, not in dictionary mode, and
///   no accessor recorded for the name — so the lookup reaches the prototype;
/// * the prototype holds the name as an own DATA property (no accessor; a
///   dictionary-mode prototype is not scanned) whose value is a closure over
///   exactly this method's thunk.
///
/// Any user change on either side — `t.unref = f`, `Object.setPrototypeOf`,
/// `Timeout.prototype.unref = f`, a getter via `defineProperty`, `delete` —
/// fails one of those checks and the call takes the tower as before. The
/// answer is the thunk's own, so the two paths cannot disagree.
pub(crate) unsafe fn try_timer_method_fast_dispatch(object: f64, name: &[u8]) -> Option<f64> {
    let thunk = match name {
        b"unref" => timer_proto_unref_thunk as *const u8,
        b"ref" => timer_proto_ref_thunk as *const u8,
        b"hasRef" => timer_proto_has_ref_thunk as *const u8,
        b"refresh" => timer_proto_refresh_thunk as *const u8,
        _ => return None,
    };
    let (id, is_immediate) = timer_handle_parts(object)?;
    let obj = (object.to_bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
    let slot = if is_immediate {
        &IMMEDIATE_PROTOTYPE_PTR
    } else {
        &TIMEOUT_PROTOTYPE_PTR
    };
    let proto = slot.load(Ordering::Acquire) as *const crate::object::ObjectHeader;
    if proto.is_null() {
        return None;
    }
    // `timer_handle_parts` proved `meta` non-null.
    if (*(*obj).meta).prototype != crate::value::js_nanbox_pointer(proto as i64).to_bits() {
        return None;
    }
    let name_str = std::str::from_utf8_unchecked(name);
    if crate::object::dictionary::is_dictionary(obj)
        || crate::object::shapes::object_shape_descriptor(obj)?.logical_key_count != 0
        || crate::object::descriptor_state::may_have_descriptor_entry(obj as usize, name_str, true)
    {
        return None;
    }
    if crate::object::dictionary::is_dictionary(proto)
        || crate::object::descriptor_state::may_have_descriptor_entry(
            proto as usize,
            name_str,
            true,
        )
    {
        return None;
    }
    let descriptor = crate::object::shapes::object_shape_descriptor(proto)?;
    let keys = descriptor.keys as usize as *const crate::array::ArrayHeader;
    if keys.is_null() || !crate::value::addr_class::is_above_handle_band(keys as usize) {
        return None;
    }
    let (slots, slot_len) = crate::object::keys_array_dense_slots_resolved(keys);
    let key_count = (descriptor.logical_key_count as usize).min(slot_len);
    let index = (0..key_count).find(|&i| {
        let key = crate::JSValue::from_bits((*slots.add(i)).to_bits());
        crate::string::js_string_key_matches_bytes(key, name)
    })?;
    let value = crate::object::js_object_get_field(proto, index as u32);
    if !value.is_pointer() {
        return None;
    }
    let closure = value.as_pointer::<crate::closure::ClosureHeader>();
    if !crate::closure::is_closure_ptr(closure as usize)
        || crate::closure::get_valid_func_ptr(closure) != thunk
    {
        return None;
    }
    // The thunks' bodies, with the receiver already in hand.
    Some(match name {
        b"unref" => {
            js_timer_unref(id);
            object
        }
        b"ref" => {
            js_timer_ref(id);
            object
        }
        b"hasRef" => f64::from_bits(crate::value::JSValue::bool(js_timer_has_ref(id) != 0).bits()),
        _ => {
            js_timer_refresh(id);
            object
        }
    })
}

fn clear_every_kind(id: i64) {
    clearTimeout(id);
    clearInterval(id);
    clearImmediate(id);
}

extern "C" fn timer_proto_close_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let (this, id) = timer_receiver();
    if let Some(id) = id {
        clear_every_kind(id);
    }
    this
}

/// `t[Symbol.dispose]()` — `using t = setTimeout(...)` clears the timer (#1213).
extern "C" fn timer_proto_dispose_thunk(_c: *const crate::closure::ClosureHeader) -> f64 {
    let (_, id) = timer_receiver();
    if let Some(id) = id {
        clear_every_kind(id);
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `+timeout` — node's `Timeout[Symbol.toPrimitive]` yields the timer id.
/// Installed on `Timeout.prototype` ONLY: node's `Immediate` has no numeric
/// conversion, so `+setImmediate(...)` must stay `NaN` (#10542).
extern "C" fn timer_proto_to_primitive_thunk(
    _c: *const crate::closure::ClosureHeader,
    _hint: f64,
) -> f64 {
    let (_, id) = timer_receiver();
    match id {
        Some(id) => id as f64,
        None => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

/// `t.constructor` names `Timeout` / `Immediate` in node, and the constructor
/// is not usable directly. This stands in for it so `t.constructor.name` is
/// answered by an ordinary prototype property instead of the fabricated
/// `{ name }` object the handle path had to synthesize per read.
extern "C" fn timer_ctor_thunk(_c: *const crate::closure::ClosureHeader, _a: f64) -> f64 {
    throw_timer_type_error(b"Timeout is not a constructor")
}

fn install_timer_symbol_method(
    proto: *mut crate::object::ObjectHeader,
    symbol_name: &str,
    display_name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    let sym = crate::symbol::well_known_symbol(symbol_name);
    if sym.is_null() {
        return;
    }
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    crate::object::native_module::set_bound_native_closure_name(closure, display_name);
    crate::object::native_module::set_builtin_closure_length(closure as usize, arity);
    crate::object::native_module::set_builtin_closure_non_constructable(closure as usize);
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            crate::value::js_nanbox_pointer(proto as i64),
            f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits()),
            crate::value::js_nanbox_pointer(closure as i64),
        );
    }
    crate::symbol::set_symbol_property_attrs(
        proto as usize,
        sym as usize,
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

fn install_timer_constructor(proto: *mut crate::object::ObjectHeader, name: &str) {
    let closure = crate::closure::js_closure_alloc(timer_ctor_thunk as *const u8, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(timer_ctor_thunk as *const u8, 0);
    crate::object::native_module::set_bound_native_closure_name(closure, name);
    crate::object::native_module::set_builtin_closure_length(closure as usize, 0);
    let key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
    crate::object::js_object_set_field_by_name(
        proto,
        key,
        crate::value::js_nanbox_pointer(closure as i64),
    );
    // Spec shape for a `constructor` property: writable, NOT enumerable,
    // configurable — so it stays out of `Object.keys(proto)` and `for...in`.
    crate::object::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

/// Build both prototypes into their rooted slots. Idempotent; lazy, because a
/// timer-free program must not pay for them.
fn build_timer_prototypes() {
    // Raw locals stay stable across the allocating installs below, exactly as
    // the iterator tower does (#7251).
    let _no_move = crate::gc::GcSuppressScope::new();
    for (name, slot, is_immediate) in [
        ("Timeout", &TIMEOUT_PROTOTYPE_PTR, false),
        ("Immediate", &IMMEDIATE_PROTOTYPE_PTR, true),
    ] {
        if slot.load(Ordering::Acquire) != 0 {
            continue;
        }
        let proto = crate::object::js_object_alloc(0, 0);
        if proto.is_null() {
            return;
        }
        crate::object::install_proto_method(proto, "ref", timer_proto_ref_thunk as *const u8, 0);
        crate::object::install_proto_method(
            proto,
            "unref",
            timer_proto_unref_thunk as *const u8,
            0,
        );
        crate::object::install_proto_method(
            proto,
            "hasRef",
            timer_proto_has_ref_thunk as *const u8,
            0,
        );
        // Measured against node 26.8.1: `Timeout.prototype` owns exactly
        // `close, constructor, hasRef, ref, refresh, unref`, while
        // `Immediate.prototype` owns only `constructor, hasRef, ref, unref` —
        // an Immediate has no `refresh` and no `close`, and no
        // `Symbol.toPrimitive` either (`+setImmediate(...)` is NaN, #10542).
        // Both carry `Symbol.dispose`.
        if !is_immediate {
            crate::object::install_proto_method(
                proto,
                "refresh",
                timer_proto_refresh_thunk as *const u8,
                0,
            );
            crate::object::install_proto_method(
                proto,
                "close",
                timer_proto_close_thunk as *const u8,
                0,
            );
        }
        install_timer_constructor(proto, name);
        install_timer_symbol_method(
            proto,
            "dispose",
            "[Symbol.dispose]",
            timer_proto_dispose_thunk as *const u8,
            0,
        );
        if !is_immediate {
            install_timer_symbol_method(
                proto,
                "toPrimitive",
                "[Symbol.toPrimitive]",
                timer_proto_to_primitive_thunk as *const u8,
                1,
            );
        }
        // No `Symbol.toStringTag`: node brands both as `[object Object]`.
        slot.store(proto as i64, Ordering::Release);
    }
}

fn timer_prototype(kind: CallbackTimerKind) -> *mut crate::object::ObjectHeader {
    let slot = match kind {
        CallbackTimerKind::Immediate => &IMMEDIATE_PROTOTYPE_PTR,
        _ => &TIMEOUT_PROTOTYPE_PTR,
    };
    let existing = slot.load(Ordering::Acquire);
    if existing != 0 {
        return existing as *mut crate::object::ObjectHeader;
    }
    build_timer_prototypes();
    slot.load(Ordering::Acquire) as *mut crate::object::ObjectHeader
}

/// Wrap a freshly scheduled timer id in the JS-visible handle object. The id
/// itself stays the runtime's internal currency (every `CallbackTimer` record,
/// every `js_timer_*` entry point and every internal `unref` still speaks ids);
/// only what crosses into JS changes.
pub(super) fn timer_object(id: i64, kind: CallbackTimerKind) -> i64 {
    let class_id = match kind {
        CallbackTimerKind::Immediate => IMMEDIATE_CLASS_ID,
        _ => TIMEOUT_CLASS_ID,
    };
    let obj = crate::object::js_object_alloc(class_id, 0);
    if obj.is_null() {
        return 0;
    }
    // Building the prototype allocates (lazily, on the first timer of a
    // program) and `GC_TYPE_OBJECT` is movable, so the instance is re-read
    // through its handle after each allocating step.
    let scope = crate::gc::RuntimeHandleScope::new();
    let handle = scope.root_raw_mut_ptr(obj);
    let proto = timer_prototype(kind);
    debug_assert!(
        !proto.is_null(),
        "a timer handle must carry its prototype, or it has no methods"
    );
    if !proto.is_null() {
        handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| {
            crate::object::prototype_chain::object_link_class_default_prototype(
                obj as usize,
                crate::value::js_nanbox_pointer(proto as i64).to_bits(),
            );
        });
    }
    handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| unsafe {
        let meta = crate::object::object_meta_ensure(obj);
        debug_assert!(!meta.is_null(), "a timer handle must carry its meta");
        if !meta.is_null() {
            (*meta).native_state = timer_state_word(id, kind);
        }
    });
    handle.with_mut_ptr::<crate::object::ObjectHeader, _>(|obj| obj as i64)
}
