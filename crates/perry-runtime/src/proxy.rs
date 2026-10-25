//! Minimal Proxy runtime support.
//!
//! A `Proxy` wraps a `target` (any JSValue, NaN-boxed f64) and a `handler`
//! (an object whose own fields include optional trap functions: `get`, `set`,
//! `has`, `deleteProperty`, `apply`, `construct`). Traps are closures created
//! in user code.
//!
//! A collecting, nonmoving GC cell owns target, handler, flags and a props
//! bag. Its POINTER_TAG payload has bit zero set; ordinary aligned receivers
//! reject that brand with one bit test, and the positive path proves GC kind
//! Proxy before reading the cell. The apparent receiver prefix stays zero. A revoked proxy has its `revoked` flag
//! flipped AND its target/handler slots detached (nulled) so the wrapped
//! object graphs can be collected; subsequent operations throw the
//! revoked-proxy TypeError.
//!
//! We deliberately do NOT patch generic object.rs/field dispatch — Perry
//! codegen rewrites known Proxy locals to ProxyGet/ProxySet/etc. variants at
//! HIR lowering time, which route through the entry points here.

mod apply_construct;
pub use apply_construct::{call_proxy_value_with_this, js_proxy_apply, js_proxy_construct};
pub(crate) use apply_construct::{is_callable_function, is_constructor_function};
mod get;
pub use get::js_proxy_get;
pub(crate) use get::{proxy_get_from_prototype, proxy_get_with_receiver};
mod has_delete;
pub(crate) use has_delete::reflect_ordinary_delete_property_key;
pub use has_delete::{js_proxy_delete, js_proxy_has};
mod invariants;
#[cfg(test)]
mod receiver_definition_tests;
mod set_interceptor;
pub(crate) use set_interceptor::set_interceptor;
mod put_value;
pub use put_value::{js_proxy_set, js_put_value_set, js_put_value_set_dynamic};
pub(crate) use put_value::{proxy_set_with_receiver, IC_SLOT_OVERFLOW_BIT};
mod json;
pub(crate) mod metadata;
mod own_keys;
mod prototype;
mod reflect;
mod reflect_misc;
pub(crate) use reflect_misc::js_proxy_get_prototype_of;
pub use reflect_misc::{
    js_reflect_apply, js_reflect_construct, js_reflect_define_property,
    js_reflect_get_prototype_of, js_reflect_is_extensible, js_reflect_own_keys,
    js_reflect_prevent_extensions,
};

pub use own_keys::js_proxy_own_keys;
pub(crate) use own_keys::{
    proxy_enum_own_keys, proxy_own_property_names, proxy_own_property_symbols,
};
pub use prototype::js_reflect_set_prototype_of;

pub(crate) use json::{
    js_proxy_checked_target, js_proxy_checked_target_for_is_array, js_proxy_own_keys_for_json,
};
pub(crate) use reflect::js_reflect_get_with_explicit_receiver;
pub use reflect::{
    js_reflect_delete, js_reflect_get, js_reflect_get_own_property_descriptor, js_reflect_has,
    js_reflect_set,
};

/// Owned proxy fields. Revocation clears both JSValue edges; every trap path
/// checks the revocation flag before touching either slot. Collection follows
/// these edges only when it reaches the proxy cell, like any other object.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProxyEntry {
    // Both receiver words stay zero, so an inline ShapeId guard (+4) or a
    // receiver-word probe never matches a Proxy cell.
    _receiver_words: [u64; 2],
    pub target: f64,  // NaN-boxed target value; 0 bits once revoked
    pub handler: f64, // NaN-boxed handler object (raw f64 bits preserved); 0 bits once revoked
    pub(crate) props: *mut crate::object::ObjectHeader,
    pub revoked: bool,
    /// Creation-time presence of [[Construct]], independent of [[Call]].
    pub constructable: bool,
}

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// A Proxy is a nonmoving GC cell of kind `GC_TYPE_PROXY`, boxed under
/// `POINTER_TAG` like any object (honest pointers), so its header byte is the
/// whole proof. Returns the cell address.
pub(crate) fn canonical_address(proxy_boxed: f64) -> Option<usize> {
    lookup(proxy_boxed).map(|addr| addr as usize)
}
#[inline]
fn lookup(proxy_boxed: f64) -> Option<u64> {
    let bits = proxy_boxed.to_bits();
    let header = crate::value::addr_class::pointer_value_header(bits)?;
    is_proxy_header(header).then_some(bits & POINTER_MASK)
}
/// The Proxy brand: kind and the cell's exact extent. The extent is only
/// compared once the kind byte matches, so ordinary receivers pay one compare.
#[inline(always)]
pub(crate) fn is_proxy_header(header: &crate::gc::GcHeader) -> bool {
    header.obj_type == crate::gc::GC_TYPE_PROXY
        && header.size as usize == crate::gc::GC_HEADER_SIZE + std::mem::size_of::<ProxyEntry>()
}
fn entry(id: u64) -> Option<ProxyEntry> {
    // `id` comes from lookup, and no allocation occurs in this leaf. The
    // collecting-holder class preserves the cell throughout a native call.
    Some(unsafe { *(id as *const ProxyEntry) })
}
pub(crate) fn property_bag(owner: usize) -> *mut crate::object::ObjectHeader {
    lookup(f64::from_bits(POINTER_TAG | owner as u64)).map_or(std::ptr::null_mut(), |id| unsafe {
        (*(id as *const ProxyEntry)).props
    })
}
pub(crate) unsafe fn property_bag_ensure(owner: usize) -> *mut crate::object::ObjectHeader {
    let _publication = crate::gc::GcSuppressScope::new();
    let id = lookup(f64::from_bits(POINTER_TAG | owner as u64)).expect("live Proxy owner");
    let cell = id as *mut ProxyEntry;
    if (*cell).props.is_null() {
        let bag = crate::object::js_object_alloc(0, 0);
        crate::gc::runtime_store_gc_heap_word_slot(
            id as usize,
            std::ptr::addr_of_mut!((*cell).props) as usize,
            bag as u64,
        );
    }
    (*cell).props
}

/// Keep a proxy id visible to any full collection that starts while a native
/// proxy operation is running. The returned scope owns the slot; the handle
/// itself need not be retained because slots live until the scope is dropped.
fn pin_proxy_for_native_call(proxy_boxed: f64) -> crate::gc::RuntimeHandleScope {
    let scope = crate::gc::RuntimeHandleScope::new();
    let _ = scope.root_nanbox_f64(proxy_boxed);
    scope
}

fn proxy_arg_is_object(value: f64) -> bool {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == (crate::value::HANDLE_TAG >> 48) {
        return true;
    }
    // POINTER_TAG heap value (object / function / array).
    if top == 0x7FFD {
        let ptr = (bits & POINTER_MASK) as usize;
        if ptr < 0x1000 {
            return false;
        }
        // A Symbol is a POINTER_TAG value too (registered side-table), but it
        // is a primitive, not an object — `new Proxy(Symbol(), {})` and
        // `new Proxy({}, Symbol())` must throw TypeError.
        if crate::symbol::is_registered_symbol(ptr) {
            return false;
        }
        return true;
    }
    // Module-level raw-I64 object/array pointers (top16 == 0).
    if top == 0 && bits > 0x10000 {
        return true;
    }
    // Class refs (INT32-tagged constructors, top16 == 0x7FFE) are callable
    // objects and are valid proxy targets/handlers.
    if top == 0x7FFE {
        return true;
    }
    false
}

fn throw_proxy_non_object() -> ! {
    let msg = "Cannot create proxy with a non-object as target or handler";
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed)
}

/// Creation-time callability snapshot for a proxy's target: a nested proxy
/// contributes ITS creation-time snapshot (per spec, each proxy's [[Call]]
/// presence is fixed when that proxy is created), anything else is asked
/// directly.
fn target_callable_at_creation(target: f64) -> bool {
    match lookup(target) {
        Some(id) => unsafe {
            crate::gc::header_from_trusted_user_ptr(id as *const u8)
                .as_ref()
                .unwrap()
                ._reserved
                & crate::gc::GC_OBJECT_CALLABLE
                != 0
        },
        None => crate::object::value_is_callable(target),
    }
}

#[no_mangle]
pub extern "C" fn js_proxy_new(target: f64, handler: f64) -> f64 {
    #[cfg(perry_contract_checks)]
    crate::contracts::hard_value(target, "new Proxy");
    // #2846: validate both arguments are objects before allocating.
    if !proxy_arg_is_object(target) || !proxy_arg_is_object(handler) {
        throw_proxy_non_object();
    }
    let callable = target_callable_at_creation(target);
    let constructable = is_constructor_function(target);
    let _publication = crate::gc::GcSuppressScope::new();
    let cell = crate::arena::arena_alloc_gc_collecting_holder(
        std::mem::size_of::<ProxyEntry>(),
        8,
        crate::gc::GC_TYPE_PROXY,
    )
    .cast::<ProxyEntry>();
    unsafe {
        let header = crate::gc::header_from_trusted_user_ptr(cell.cast()).cast_mut();
        if callable {
            (*header)._reserved |= crate::gc::GC_OBJECT_CALLABLE;
        }
        debug_assert_eq!(
            perry_abi::gc_header_word_is_callable((*header).kind_facts_word()),
            callable,
            "Proxy [[Call]] must agree with its creation-time target"
        );
        // GC_STORE_AUDIT(INIT): Fresh ProxyEntry under GcSuppressScope begins with target/handler numeric0 and props=null. Real target and handler are subsequently installed through runtime_store_gc_jsvalue_slot (proxy.rs210-247).
        std::ptr::write(
            cell,
            ProxyEntry {
                _receiver_words: [0; 2],
                target: 0.0,
                handler: 0.0,
                props: std::ptr::null_mut(),
                revoked: false,
                constructable,
            },
        );
        crate::gc::runtime_store_gc_jsvalue_slot(
            cell as usize,
            std::ptr::addr_of_mut!((*cell).target) as usize,
            target.to_bits(),
        );
        crate::gc::runtime_store_gc_jsvalue_slot(
            cell as usize,
            std::ptr::addr_of_mut!((*cell).handler) as usize,
            handler.to_bits(),
        );
    }
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(crate::hot_diag::ReceiverReprFamily::Proxy);
    }
    f64::from_bits(POINTER_TAG | cell as u64)
}

/// Revoke a proxy. Subsequent operations will return TAG_UNDEFINED or fire an
/// exception where the compiler inserts one. Detaches the entry's
/// target/handler (stores 0 bits — spec: [[ProxyTarget]]/[[ProxyHandler]]
/// become null) so the graphs they root can be collected; every trap path
/// throws the revoked-proxy TypeError off the `revoked` flag before reading
/// either slot. Idempotent.
#[no_mangle]
pub extern "C" fn js_proxy_revoke(proxy_boxed: f64) {
    if let Some(id) = lookup(proxy_boxed) {
        unsafe {
            let cell = &mut *(id as *mut ProxyEntry);
            cell.revoked = true;
            crate::gc::runtime_store_gc_jsvalue_slot(
                id as usize,
                std::ptr::addr_of_mut!(cell.target) as usize,
                0,
            );
            crate::gc::runtime_store_gc_jsvalue_slot(
                id as usize,
                std::ptr::addr_of_mut!(cell.handler) as usize,
                0,
            );
        }
    }
}

/// Query whether `proxy_boxed` is a currently-revoked proxy. Returns 1 if so.
#[no_mangle]
pub extern "C" fn js_proxy_is_revoked(proxy_boxed: f64) -> i32 {
    if let Some(id) = lookup(proxy_boxed) {
        return entry(id)
            .map(|e| if e.revoked { 1i32 } else { 0 })
            .unwrap_or(0);
    }
    0
}

/// Query whether the given NaN-boxed value is a Proxy instance. Returns 1/0.
#[no_mangle]
pub extern "C" fn js_proxy_is_proxy(value: f64) -> i32 {
    if lookup(value).is_some() {
        1
    } else {
        0
    }
}

/// `IsArray`'s Proxy branch (ECMA-262 §7.2.2). If `value` is a live Proxy,
/// returns `Some(target)` so the caller can recurse on the target; if the Proxy
/// has been revoked, throws a `TypeError` (does not return). Returns `None` for
/// any non-Proxy value, so the caller falls back to its ordinary array check.
pub(crate) fn is_array_proxy_step(value: f64) -> Option<f64> {
    let id = lookup(value)?;
    let (target, revoked) = entry(id)
        .map(|e| (e.target, e.revoked))
        .unwrap_or((f64::from_bits(TAG_UNDEFINED), false));
    if revoked {
        revoked_return_with_message("Cannot perform 'IsArray' on a proxy that has been revoked");
    }
    Some(target)
}

/// Whether a Proxy value's (possibly nested) [[ProxyTarget]] is callable —
/// the predicate behind `typeof proxyOfFn === "function"` and
/// `Function.prototype.toString` accepting a proxy receiver. Reads the
/// creation-time header fact (which already resolved through nested
/// proxies), so callability survives revocation (per spec, `typeof` of a
/// revoked proxy is unchanged) even though revoke nulls the recorded target.
pub(crate) fn proxy_wraps_callable(value: f64) -> bool {
    match lookup(value) {
        Some(id) => unsafe {
            crate::gc::header_from_trusted_user_ptr(id as *const u8)
                .as_ref()
                .unwrap()
                ._reserved
                & crate::gc::GC_OBJECT_CALLABLE
                != 0
        },
        None => crate::object::value_is_callable(value),
    }
}

/// Return the proxy's target (for Proxy.revocable.proxy revocation checks).
/// A revoked proxy's target slot is detached (0 bits) — report it as
/// `undefined`, the same "no target" convention non-proxies get.
#[no_mangle]
pub extern "C" fn js_proxy_target(proxy_boxed: f64) -> f64 {
    if let Some(id) = lookup(proxy_boxed) {
        return entry(id)
            .map(|e| e.target)
            .filter(|t| t.to_bits() != 0)
            .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Return the proxy's handler for `util.inspect(..., { showProxy: true })`.
/// A revoked proxy's handler slot is detached (0 bits) — report `undefined`.
#[no_mangle]
pub extern "C" fn js_proxy_handler(proxy_boxed: f64) -> f64 {
    if let Some(id) = lookup(proxy_boxed) {
        return entry(id)
            .map(|e| e.handler)
            .filter(|h| h.to_bits() != 0)
            .unwrap_or(f64::from_bits(TAG_UNDEFINED));
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// Helper: fetch the trap closure from the handler object by name. Returns
/// TAG_UNDEFINED if the handler has no such trap.
fn handler_trap(handler: f64, trap_name: &str) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let handler = scope.root_nanbox_f64(handler);
    let key = crate::string::js_string_from_bytes(trap_name.as_ptr(), trap_name.len() as u32);
    let obj_ptr = extract_pointer(handler.get_nanbox_f64().to_bits()) as *const crate::ObjectHeader;
    if obj_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // The ordinary lookup roots and reloads its operands around a getter.
    // Its native-alias fallback needs the current receiver and a stable name,
    // because an undefined-returning getter may have moved both heap operands.
    let result = crate::object::js_object_get_field_by_name(obj_ptr, key);
    if result.bits() == TAG_UNDEFINED {
        if let Some(value) = crate::object::native_this_alias::alias_forward_property_read(
            handler.get_nanbox_f64(),
            trap_name,
        ) {
            return value;
        }
    }
    f64::from_bits(result.bits())
}

/// Raise a "proxy revoked" TypeError via `js_throw`. Does not return.
fn revoked_return() -> f64 {
    revoked_return_with_message("Cannot perform operation on a proxy that has been revoked")
}

fn revoked_return_with_message(msg: &str) -> f64 {
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed);
}

fn is_callable(value: f64) -> bool {
    // Treat any pointer-tagged value as potentially callable. Inside the
    // closure call the runtime will no-op if the pointer isn't a closure.
    let bits = value.to_bits();
    let tag = bits & !POINTER_MASK;
    tag == POINTER_TAG && (bits & POINTER_MASK) != 0
}

fn closure_from(value: f64) -> *const crate::ClosureHeader {
    let bits = value.to_bits();
    ((bits & POINTER_MASK) as usize) as *const crate::ClosureHeader
}

/// Coerce a trap return value to a NaN-boxed boolean (`ToBoolean`), as the
/// `Reflect.{set,deleteProperty,defineProperty,preventExtensions}` paths must
/// return the trap's boolean result rather than discarding it.
fn nanbox_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

fn coerce_trap_bool(value: f64) -> f64 {
    nanbox_bool(crate::value::js_is_truthy(value) != 0)
}

/// Invoke a present (already-confirmed-callable) handler trap with the handler
/// bound as the trap's `this` (ECMA-262: traps are called as
/// `Call(trap, handler, args)`). Object-literal/method traps read `this` from a
/// reserved closure slot, while free-function traps read the `this`
/// parameter; we supply both so either style observes the handler. Mirrors
/// the apply/construct/getOwnPropertyDescriptor trap-call dance, which the
/// per-trap paths (get/set/has/deleteProperty/defineProperty/…) previously
/// skipped — they called the trap with the wrong `this` and, for get/set,
/// dropped the trailing `receiver` argument.
fn call_trap(handler: f64, trap: f64, args: &[f64]) -> f64 {
    let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
    let handler = this_scope.root_nanbox_f64(handler);
    let rebound =
        crate::closure::clone_closure_rebind_this(trap.to_bits(), handler.get_nanbox_f64());
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return throw_type_error("proxy trap is not a function");
    }
    let undef = f64::from_bits(TAG_UNDEFINED);
    let a = |i: usize| -> f64 { args.get(i).copied().unwrap_or(undef) };
    let this = crate::closure::JsThis::from_f64(handler.get_nanbox_f64());
    match args.len() {
        0 => crate::closure::js_closure_call0(closure, this),
        1 => crate::closure::js_closure_call1(closure, this, a(0)),
        2 => crate::closure::js_closure_call2(closure, this, a(0), a(1)),
        3 => crate::closure::js_closure_call3(closure, this, a(0), a(1), a(2)),
        _ => crate::closure::js_closure_call4(closure, this, a(0), a(1), a(2), a(3)),
    }
}

/// Throw `TypeError: Reflect.<op> called on non-object`. Does not return.
fn reflect_non_object_typeerror(op: &str) -> f64 {
    let msg = format!("Reflect.{op} called on non-object");
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed);
}

/// Throw a `TypeError` with an arbitrary message. Does not return.
fn throw_type_error(msg: &str) -> f64 {
    let msg_handle = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_handle);
    let boxed = f64::from_bits(POINTER_TAG | ((err as u64) & POINTER_MASK));
    crate::exception::js_throw(boxed)
}

/// `String(value)` rendering of a JS value, for diagnostic messages that
/// embed the offending value (e.g. Node's `"1 is not a constructor"` and
/// the proxy construct-trap `"… non-object ('1')"`). Returns an empty
/// string on a null/unrenderable value. (#2768)
pub(crate) fn value_display_string(value: f64) -> String {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let str_ptr = crate::value::js_jsvalue_to_string(value);
    if str_ptr.is_null() {
        return String::new();
    }
    let nb = f64::from_bits(crate::value::STRING_TAG | (str_ptr as u64 & POINTER_MASK));
    if let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(nb, &mut scratch) {
        if !ptr.is_null() && len > 0 {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            return String::from_utf8_lossy(bytes).into_owned();
        }
    }
    String::new()
}

/// #6828/#10482: Annex B §B.3.1 `set __proto__` semantics — shared by the
/// real accessor descriptor installed on `Object.prototype`
/// (`object/global_this/proto_methods.rs`'s setter closure, reached via
/// `own_set_descriptor` + `call_setter_with_receiver` above) and this
/// function's own caller (the walk's defensive fallback for the same key).
/// Both must behave identically, so both call this one implementation
/// instead of keeping the logic written out twice.
///
/// Per spec: a non-object/non-null `value` or a non-object `receiver` is
/// silently ignored (no throw, unlike `Object.setPrototypeOf`); a genuine
/// `[[SetPrototypeOf]]` failure (cyclic / non-extensible) still throws via
/// `js_object_set_prototype_of` itself, matching `Object.setPrototypeOf`'s
/// failure behavior for that case.
pub(crate) fn legacy_dunder_proto_set(receiver: f64, value: f64) {
    let value_bits = value.to_bits();
    let valid_proto = value_bits == TAG_NULL
        || lookup(value).is_some()
        || crate::object::class_ref_id(value).is_some()
        || unsafe { crate::object::value_is_object_like(value) };
    if valid_proto && reflect_value_is_object(receiver) {
        crate::object::js_object_set_prototype_of(receiver, value);
    }
}

/// Is `value` a Reflect-acceptable object? Heap objects, class refs (callable
/// constructors), and proxies all count. Primitives / null / undefined do not.
pub(crate) fn reflect_value_is_object(value: f64) -> bool {
    // One shared Type(value) is Object answer: a POINTER_TAG word names a
    // GC-headed object (honest pointers), so its header byte decides, and
    // native ids are objects. Raw untagged words keep the allocator proof;
    // class refs (INT32-tagged constructors) are callable objects.
    if crate::builtins::arithmetic::js_value_is_object(value) {
        return true;
    }
    let top16 = value.to_bits() >> 48;
    if top16 == 0 {
        return crate::object::js_value_is_heap_object(value);
    }
    top16 == 0x7FFE
}

/// `CreateListFromArrayLike(value)` — collect indexed `0..length` properties of
/// an array-like object into an owned `Vec<f64>`. Throws `TypeError` for a
/// non-object `value`, matching Node's `CreateListFromArrayLike called on
/// non-object`. Plain Arrays use the fast array accessors; any other object
/// reads `.length` then `[0]..[length-1]` via the field getter.
fn create_list_from_array_like(value: f64) -> Vec<f64> {
    // Fast path: a real Array.
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let is_pointer = top16 == 0x7FFD || (top16 == 0 && bits > 0x10000);
    if is_pointer {
        let ptr = (bits & POINTER_MASK) as usize;
        // `arguments` is an ordinary ObjectHeader backed by the arguments
        // registry, not a GC_TYPE_ARRAY. Reading it through the generic object
        // field path below loses its indexed bindings, so
        // `Reflect.apply(target, thisArg, arguments)` constructed an empty
        // argument list. Next 16's ProxyTracer forwards startActiveSpan with
        // exactly that shape; its callback was consequently never invoked and
        // the production App Route request remained pending (#8036).
        if let Some(values) = unsafe {
            crate::object::arguments_object_to_vec(ptr as *const crate::object::ObjectHeader)
        } {
            return values;
        }
        // #7531: `value` is `argumentsList` from `Reflect.apply(target,
        // thisArg, argumentsList)` / `Reflect.construct` -- caller-supplied,
        // so it can be a fetch/zlib/proxy/common-registry handle id under
        // the same POINTER_TAG as a real Array. The old floor
        // (`GC_HEADER_SIZE + 0x1000`) sits below every handle band and had
        // no other guard before the deref below -- a handle reached
        // `addr - GC_HEADER_SIZE` unconditionally.
        if crate::value::addr_class::is_plausible_heap_addr(ptr) {
            unsafe {
                let gc =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY {
                    let arr = ptr as *const crate::array::ArrayHeader;
                    let len = crate::array::js_array_length(arr) as usize;
                    let mut out = Vec::with_capacity(len);
                    for i in 0..len {
                        let v = crate::array::js_array_get(arr, i as u32);
                        out.push(f64::from_bits(v.bits()));
                    }
                    return out;
                }
            }
        }
    }
    if !reflect_value_is_object(value) {
        throw_type_error("CreateListFromArrayLike called on non-object");
    }
    // General array-like object: read `.length`, then `[0]..[length-1]`.
    let len_key = crate::string::js_string_from_bytes(b"length".as_ptr(), 6);
    let len_val = {
        let obj_ptr = extract_pointer(value.to_bits()) as *const crate::ObjectHeader;
        if obj_ptr.is_null() {
            f64::from_bits(TAG_UNDEFINED)
        } else {
            crate::object::js_object_get_field_by_name_f64(obj_ptr, len_key)
        }
    };
    let len_f = f64::from_bits(len_val.to_bits());
    let len = if len_f.is_finite() && len_f > 0.0 {
        len_f as usize
    } else {
        0
    };
    // Every index key allocates, and a property read can run a getter: the
    // source object and the elements read so far must survive both, so the
    // list handed back holds their post-collection addresses (#10532 review).
    // Only values a collection can relocate take a handle — an all-primitive
    // argument list pays one scope and a tag test per element.
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_nanbox_f64(value);
    let mut out = Vec::with_capacity(len);
    let mut moved = Vec::new();
    for i in 0..len {
        let idx_str = i.to_string();
        crate::gc::collection_point("reflect.list_from_array_like.index_key");
        let key = crate::string::js_string_from_bytes(idx_str.as_ptr(), idx_str.len() as u32);
        let obj_ptr =
            extract_pointer(source.get_nanbox_f64().to_bits()) as *const crate::ObjectHeader;
        let v = crate::object::js_object_get_field_by_name_f64(obj_ptr, key);
        let word = crate::value::JSValue::from_bits(v.to_bits());
        if word.is_pointer() || word.is_string() || word.is_bigint() {
            moved.push((i, scope.root_nanbox_f64(v)));
        }
        out.push(v);
    }
    for (index, handle) in moved {
        out[index] = handle.get_nanbox_f64();
    }
    out
}

/// Invoke a callable `f64` value with the supplied positional args and an
/// explicit `thisArg` binding, throwing `TypeError` if `f` is not callable.
/// Used by `Reflect.apply`. `thisArg` is passed as the `this` parameter so free
/// functions reading `this` observe it.
fn call_with_this_and_args(f: f64, this_arg: f64, args: &[f64]) -> f64 {
    // A concise/object-literal method reads `this` from a baked capture slot,
    // not only the `this` parameter; the shared explicit-`this` forwarder
    // rebinds it to the `Reflect.apply` receiver (no-op for arrows / plain fns
    // / bound fns) and holds the callee, receiver and arguments in handles
    // when that rebind clones (#10532 review). The receiver is passed as given.
    unsafe {
        crate::closure::forward_with_explicit_this(
            f,
            this_arg,
            args,
            crate::closure::ReceiverBinding::AsGiven,
            |call| dispatch_with_explicit_this(call.target, call.this, call.args),
        )
    }
}

/// Invoke an already-rebound callable with an explicit `this`. Nothing here
/// allocates before the callee runs, so the arguments need no protection.
fn dispatch_with_explicit_this(f: f64, this_arg: f64, args: &[f64]) -> f64 {
    let closure = closure_from(f);
    if closure.is_null() {
        return throw_type_error("Reflect.apply target is not a function");
    }
    let this = crate::closure::JsThis::from_f64(this_arg);
    let a = |i: usize| -> f64 {
        args.get(i)
            .copied()
            .unwrap_or(f64::from_bits(TAG_UNDEFINED))
    };
    match args.len() {
        0 => crate::closure::js_closure_call0(closure, this),
        1 => crate::closure::js_closure_call1(closure, this, a(0)),
        2 => crate::closure::js_closure_call2(closure, this, a(0), a(1)),
        3 => crate::closure::js_closure_call3(closure, this, a(0), a(1), a(2)),
        4 => crate::closure::js_closure_call4(closure, this, a(0), a(1), a(2), a(3)),
        // #10425: this arm was `_ => js_closure_call4(…)`, so every argument
        // after the fourth was dropped. The variadic entry point owns
        // arbitrary-arity dispatch, rest bundling included. (`call_trap`
        // above keeps its catch-all: a proxy trap receives at most four.)
        n => unsafe {
            crate::closure::js_closure_call_array(closure as i64, this, args.as_ptr(), n as i64)
        },
    }
}

/// Resolve the ultimate target when a Proxy wraps a class constructor. Used
/// by method-call dispatch to bind a static method's visible `this` to the
/// Proxy receiver while retaining the target class as its lexical owner.
pub(crate) fn proxy_target_class_id(mut value: f64) -> Option<u32> {
    let mut depth = 0usize;
    while let Some(id) = lookup(value) {
        value = entry(id)
            .map(|entry| entry.target)
            .unwrap_or(f64::from_bits(TAG_UNDEFINED));
        depth += 1;
        if depth >= 32 {
            return None;
        }
    }
    if let Some(class_id) = crate::object::class_ref_id(value) {
        return Some(class_id);
    }
    if crate::object::is_class_object_value(value) {
        let raw = extract_pointer(value.to_bits()) as *const crate::ObjectHeader;
        let class_id = crate::object::js_object_get_class_id(raw);
        return (class_id != 0).then_some(class_id);
    }
    None
}

/// Extract a raw heap pointer (48-bit) from either a NaN-boxed value
/// (POINTER_TAG / STRING_TAG) or a raw i64/f64-reinterpreted pointer
/// (module-level globals store Arrays/Objects as raw I64s, not NaN-boxed).
fn extract_pointer(bits: u64) -> u64 {
    let top = bits >> 48;
    // A native id reaches the by-name/handle dispatch as its raw id.
    if top == 0x7FFD || top == 0x7FFF || top == (crate::value::HANDLE_TAG >> 48) {
        bits & POINTER_MASK
    } else if top == 0 {
        // Raw untagged pointer (module-level I64 global).
        bits
    } else {
        0
    }
}

fn small_handle_from_value(value: f64) -> Option<i64> {
    let bits = value.to_bits();
    let top = bits >> 48;
    if top == (crate::value::HANDLE_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as i64;
        if raw > 0 && (raw as usize) < crate::value::addr_class::HANDLE_BAND_MAX {
            return Some(raw);
        }
    } else if top == 0 && crate::value::addr_class::is_small_handle(bits as usize) {
        return Some(bits as i64);
    }
    None
}

fn set_handle_property(target: f64, key: f64, value: f64) -> Option<bool> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_nanbox_f64(target);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    let handle = small_handle_from_value(target.get_nanbox_f64())?;
    let Some(name) = key_to_rust_string(key.get_nanbox_f64()) else {
        // A SYMBOL-keyed write on a small native handle (e.g. the
        // @hono/node-server `incoming[wrapBodyStream] = true` on the HTTP
        // IncomingMessage handle). The handle is not a heap ObjectHeader, so
        // it has no field storage; route the write to the per-object symbol
        // side table (keyed by the handle pointer, exactly like a plain
        // object) and report success. Returning `Some(false)` here made
        // strict-mode assignment throw `TypeError: Cannot assign to read only
        // property` and 500 every POST/PUT served by Hono's node adapter.
        if unsafe { crate::symbol::js_is_symbol(key.get_nanbox_f64()) } != 0 {
            unsafe {
                crate::symbol::js_object_set_symbol_property(
                    target.get_nanbox_f64(),
                    key.get_nanbox_f64(),
                    value.get_nanbox_f64(),
                )
            };
            return Some(true);
        }
        return Some(false);
    };
    if let Some(dispatch) = crate::object::handle_property_set_dispatch() {
        unsafe { dispatch(handle, name.as_ptr(), name.len(), value.get_nanbox_f64()) };
    }
    Some(true)
}

fn target_get_property_key(target: f64, property_key: f64) -> f64 {
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        return unsafe { crate::symbol::js_object_get_symbol_property(target, property_key) };
    }
    let key_ptr =
        crate::value::js_get_string_pointer_unified(property_key) as *const crate::StringHeader;
    if key_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Class constructors use Perry's INT32-tagged ClassRef representation,
    // not a heap pointer. Preserve those bits exactly as the ordinary dynamic
    // class-property path does; pointer extraction would turn the target into
    // null and make `new Proxy(C, {}).staticMethod` read as `undefined`.
    if crate::object::class_ref_id(target).is_some() {
        return crate::object::js_object_get_field_by_name_f64(
            target.to_bits() as *const crate::ObjectHeader,
            key_ptr,
        );
    }
    let obj_ptr = extract_pointer(target.to_bits()) as *const crate::ObjectHeader;
    if obj_ptr.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    crate::object::js_object_get_field_by_name_f64(obj_ptr, key_ptr)
}

/// `Reflect.set` with an explicit receiver: OrdinarySet(target, P, V,
/// receiver), boolean result NaN-boxed.
pub(crate) fn reflect_ordinary_set_with_receiver(
    target: f64,
    property_key: f64,
    value: f64,
    receiver: f64,
) -> f64 {
    nanbox_bool(ordinary_set_with_receiver(
        target,
        property_key,
        value,
        receiver,
    ))
}

fn target_set(target: f64, key: f64, value: f64) {
    // #6935: `js_to_property_key` runs a user `Symbol.toPrimitive` / `toString`
    // / `valueOf` for an object key (and allocates for every primitive one), so
    // it can trigger a GC that **evacuates** live objects. Both the `target`
    // receiver and the `value` being written into it were raw NaN-boxed Rust
    // locals across the call — a stale target dropped the write onto a
    // forwarding stub, a stale value stored a dangling pointer in a live
    // object. `target_get` already roots; this write sibling did not.
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_u64(target.to_bits());
    let value_handle = scope.root_nanbox_f64(value);
    let property_key = unsafe { crate::object::js_to_property_key(key) };
    let property_key = scope.root_nanbox_f64(property_key).get_nanbox_f64();
    let target = f64::from_bits(target_handle.get_nanbox_u64());
    let value = value_handle.get_nanbox_f64();
    if unsafe { crate::symbol::js_is_symbol(property_key) } != 0 {
        unsafe {
            crate::symbol::js_object_set_symbol_property(target, property_key, value);
        }
        return;
    }
    // #6943 audit: this `js_string_coerce` is provably INERT and needs no
    // rooting. `js_to_property_key` returns either a Symbol — taken by the
    // early return above — or `js_nanbox_string(heap_ptr)`, i.e. an
    // already-heap `STRING_TAG` value, which `js_string_coerce` hands straight
    // back without touching the allocator.
    let key_ptr = crate::builtins::js_string_coerce(property_key) as *const crate::StringHeader;
    if crate::object::class_ref_id(target).is_some() {
        // Preserve the INT32-tagged class-ref bits so class dynamic props
        // land in CLASS_DYNAMIC_PROPS instead of being pointer-extracted to 0.
        if !key_ptr.is_null() {
            crate::object::js_object_set_field_by_name(
                target.to_bits() as *mut crate::ObjectHeader,
                key_ptr,
                value,
            );
        }
        return;
    }
    let obj_addr = extract_pointer(target.to_bits()) as usize;
    if crate::closure::is_closure_ptr(obj_addr) {
        if let Some(name) = key_to_rust_string(property_key) {
            crate::closure::closure_set_dynamic_prop(obj_addr, &name, value);
        }
        return;
    }
    let obj_ptr = obj_addr as *mut crate::ObjectHeader;
    if obj_ptr.is_null() || key_ptr.is_null() {
        return;
    }
    crate::object::js_object_set_field_by_name(obj_ptr, key_ptr, value);
}

fn raw_ptr_from_value(value: f64) -> Option<usize> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let raw = if top16 == (POINTER_TAG >> 48) {
        bits & POINTER_MASK
    } else if top16 == 0 && bits > 0x10000 {
        bits
    } else {
        return None;
    } as usize;
    // #7531: `value` is a Proxy `target`/`key` candidate -- caller-supplied,
    // so it can be a fetch/zlib/proxy/common-registry handle id. The old
    // floor (`GC_HEADER_SIZE + 0x1000`) sits below every handle band; the
    // local name `raw` also made this invisible to
    // `scripts/addr_class_inventory.py`'s `handle-floor` regex (it only
    // matches `ptr`/`addr`/`bits`-shaped identifiers), so this was debt the
    // ratchet could not see. `array_ptr_from_value` derefs
    // `raw - GC_HEADER_SIZE` right after this with only a magnitude-only
    // `is_valid_obj_ptr` guard in between, so a handle id reached that
    // deref.
    if !crate::value::addr_class::is_above_handle_band(raw) {
        return None;
    }
    Some(raw)
}

fn array_ptr_from_value(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let raw = raw_ptr_from_value(value)?;
    if crate::buffer::is_registered_buffer(raw)
        || crate::typedarray::lookup_typed_array_kind(raw).is_some()
        || !crate::object::is_valid_obj_ptr(raw as *const u8)
    {
        return None;
    }
    unsafe {
        let gc = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc).obj_type == crate::gc::GC_TYPE_ARRAY
            || (*gc).obj_type == crate::gc::GC_TYPE_LAZY_ARRAY
        {
            Some(raw as *mut crate::array::ArrayHeader)
        } else {
            None
        }
    }
}

fn key_equals(key: f64, name: &[u8]) -> bool {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return false;
    };
    if ptr.is_null() || len as usize != name.len() {
        return false;
    }
    unsafe { std::slice::from_raw_parts(ptr, len as usize) == name }
}

fn key_is_length(key: f64) -> bool {
    key_equals(key, b"length")
}

/// Does deleting `key` off `target` hit a non-configurable exotic own property
/// that lives outside the ordinary descriptor table? Covers an Array's `length`
/// and a plain (non-arrow, non-bound) function's `prototype` — both are
/// non-configurable, so `Reflect.deleteProperty` / `delete` must report failure.
fn is_non_configurable_exotic_own(target: f64, key: f64) -> bool {
    if array_ptr_from_value(target).is_some() && key_is_length(key) {
        return true;
    }
    if key_equals(key, b"prototype") {
        let raw = extract_pointer(target.to_bits()) as usize;
        if raw != 0 && crate::closure::is_closure_ptr(raw) {
            let closure = raw as *const crate::closure::ClosureHeader;
            // Arrow and bound functions have no own `prototype` slot at all, so
            // deleting it succeeds vacuously; only a plain function's `prototype`
            // is a non-configurable own property.
            if !crate::closure::closure_is_arrow(closure)
                && !crate::closure::closure_is_bound_method(closure)
                && !crate::closure::closure_body_is_non_constructor(closure)
            {
                return true;
            }
        }
    }
    false
}

fn parse_canonical_nonnegative_i32(bytes: &[u8]) -> Option<i32> {
    if bytes.is_empty() || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    let mut value = 0u32;
    for &byte in bytes {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((byte - b'0') as u32)?;
        if value > i32::MAX as u32 {
            return None;
        }
    }
    Some(value as i32)
}

fn integer_index_key(key: f64) -> Option<i32> {
    let jsval = crate::value::JSValue::from_bits(key.to_bits());
    if jsval.is_int32() {
        let index = jsval.as_int32();
        return (index >= 0).then_some(index);
    }
    if !key.is_nan() {
        return (key.is_finite() && key >= 0.0 && key.fract() == 0.0 && key <= i32::MAX as f64)
            .then_some(key as i32);
    }

    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some((ptr, len)) = crate::string::str_bytes_from_jsvalue(key, &mut scratch) else {
        return None;
    };
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    parse_canonical_nonnegative_i32(bytes)
}

fn set_integer_indexed_exotic(target: f64, key: f64, value: f64) -> bool {
    let Some(index) = integer_index_key(key) else {
        return false;
    };
    let Some(raw) = raw_ptr_from_value(target) else {
        return false;
    };
    // #8149: `ArrayBuffer` / `SharedArrayBuffer` / `DataView` are registered
    // buffers, but they are NOT integer-indexed exotic objects — `dv[0] = 7`
    // creates an ORDINARY own property in node and leaves the byte at 0.
    // Answering `false` here hands the write back to `js_put_value_set`'s
    // ordinary `[[Set]]` walk, which would bit-cast the `BufferHeader`, so
    // store the expando directly and claim the write.
    if crate::buffer::is_non_indexed_buffer_view(raw) {
        crate::buffer::buffer_set_own_prop(raw, &index.to_string(), value);
        return true;
    }
    if crate::buffer::is_registered_buffer(raw) {
        crate::buffer::js_buffer_set(raw as *mut crate::buffer::BufferHeader, index, value as i32);
        return true;
    }
    if crate::typedarray::lookup_typed_array_kind(raw).is_some() {
        crate::typedarray::js_typed_array_set(
            raw as *mut crate::typedarray::TypedArrayHeader,
            index,
            value,
        );
        return true;
    }
    false
}

#[derive(Clone, Copy)]
enum OwnSetDescriptor {
    Data { writable: bool },
    Accessor { setter_bits: u64 },
}

fn key_to_rust_string(value: f64) -> Option<String> {
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        return None;
    }
    let key_str = crate::builtins::js_string_coerce(value);
    if key_str.is_null() {
        return None;
    }
    unsafe {
        let name_ptr = (key_str as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let name_len = (*key_str).byte_len as usize;
        std::str::from_utf8(std::slice::from_raw_parts(name_ptr, name_len))
            .ok()
            .map(|s| s.to_string())
    }
}

fn property_key_to_rust_string(value: f64) -> Option<String> {
    let property_key = unsafe { crate::object::js_to_property_key(value) };
    key_to_rust_string(property_key)
}

fn own_set_descriptor(target: f64, key: f64) -> Option<OwnSetDescriptor> {
    if small_handle_from_value(target).is_some() {
        return None;
    }
    // Object model §3.2: an optional layout key whose slot holds the hole is
    // absent (`key_view`), so the walk goes on to the prototype chain.
    if target.to_bits() & !POINTER_MASK == POINTER_TAG
        && unsafe {
            crate::object::key_view::own_key_is_hole(
                extract_pointer(target.to_bits()) as *const crate::ObjectHeader,
                crate::JSValue::from_bits(key.to_bits()),
            )
        }
    {
        return None;
    }

    // Ordinary prototype holders answer from their shape's key index. A
    // missing key is an indexed absence, not a byte scan of every builtin
    // key followed by a virtual/global property lookup.
    let addr = extract_pointer(target.to_bits()) as usize;
    let indexed_owner = unsafe {
        match crate::value::addr_class::try_read_gc_header(addr) {
            Some(header) if header.obj_type == crate::gc::GC_TYPE_OBJECT => {
                // The indexed answer reads the layout alone; a prototype with
                // an overflow keeps keys and attributes in its entries (object
                // model step 4), which the complete lookup below reads.
                let meta = (*(addr as *const crate::ObjectHeader)).meta;
                (!meta.is_null()
                    && (*meta).flags & crate::object::OBJECT_META_FLAG_IS_PROTOTYPE != 0
                    && (*meta).overflow == 0)
                    .then_some(addr)
            }
            Some(header) if header.obj_type == crate::gc::GC_TYPE_ARRAY => {
                crate::object::shape_store::named_bag(
                    addr as *const crate::ObjectHeader,
                    key.to_bits(),
                )
                .map(|bag| bag as usize)
            }
            _ => None,
        }
    };
    if let Some(indexed_owner) = indexed_owner {
        if let Some(shape) = unsafe {
            crate::object::shapes::object_shape_descriptor(
                indexed_owner as *const crate::ObjectHeader,
            )
        } {
            if shape.object_kind.is_ordinary_layout() {
                let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
                if let Some(bytes) = unsafe {
                    crate::string::js_string_key_bytes(
                        crate::JSValue::from_bits(key.to_bits()),
                        &mut scratch,
                    )
                } {
                    let keys = shape.keys as usize as *const crate::ArrayHeader;
                    use crate::object::shapes::KeysIndexVerdict;
                    let verdict = if keys.is_null() {
                        // Keyless dictionary shapes do not answer own absence;
                        // the descriptor path below reads their receiver keys.
                        KeysIndexVerdict::Unindexed
                    } else {
                        unsafe {
                            let hash = crate::object::key_bytes_hash(bytes.as_ptr(), bytes.len());
                            let verdict = crate::object::shapes::shape_slot_lookup_verdict(
                                keys,
                                bytes,
                                hash,
                                shape.logical_key_count,
                                true,
                            );
                            // A shortened mutable key list retires its old
                            // index. Rebuild before claiming absence.
                            if matches!(verdict, KeysIndexVerdict::Unindexed) {
                                crate::object::shapes::shape_slot_lookup_verdict(
                                    keys,
                                    bytes,
                                    hash,
                                    shape.logical_key_count,
                                    true,
                                )
                            } else {
                                verdict
                            }
                        }
                    };
                    match verdict {
                        KeysIndexVerdict::Absent => return None,
                        KeysIndexVerdict::Unindexed => {}
                        KeysIndexVerdict::Found(slot) => {
                            let entry = unsafe { crate::object::key_attrs::keys_entry(keys, slot) };
                            if crate::object::key_attrs::entry_is_private(entry) {
                                return None;
                            }
                            if entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0 {
                                let name = std::str::from_utf8(bytes).ok()?;
                                let acc =
                                    crate::object::get_accessor_descriptor(indexed_owner, name)?;
                                return Some(OwnSetDescriptor::Accessor {
                                    setter_bits: acc.set,
                                });
                            }
                            return Some(OwnSetDescriptor::Data {
                                writable: entry & crate::object::key_attrs::ENTRY_NON_WRITABLE == 0,
                            });
                        }
                    }
                }
            }
        }
    }

    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        // `undefined` is a valid value for an existing own property. Using a
        // value read as the existence probe made the first overwrite of the
        // symbol metadata installed by an async_hooks `init` callback take the
        // absent-property path and disappear on native AsyncResource handles.
        if !unsafe { crate::symbol::has_own_symbol_property(target, key) } {
            return None;
        }
        // #10481: a symbol ACCESSOR is not a data slot — report its setter so
        // the walk runs it with the receiver instead of shadowing it.
        let (owner, sym_key) = unsafe {
            (
                crate::symbol::obj_key_from_f64(target),
                crate::symbol::sym_key_from_f64(key),
            )
        };
        if let Some((_, setter_bits)) =
            crate::symbol::symbol_accessor_descriptor_bits(owner, sym_key)
        {
            return Some(OwnSetDescriptor::Accessor { setter_bits });
        }
        // An existing symbol-keyed own data property is non-writable when the
        // receiver is frozen or its per-symbol attrs say so — so a strict
        // `obj[sym] = v` is rejected (throws) rather than silently no-op'd
        // (test262 Object/freeze/frozen-object-contains-symbol-properties-strict).
        // Mirrors the string-keyed / `set_symbol_property` guards.
        let writable = !crate::symbol::symbol_property_is_non_writable(target, key);
        return Some(OwnSetDescriptor::Data { writable });
    }

    // The null-receiver guard stays BEFORE the coercion: `key_to_rust_string`
    // can run a user `toString`, and moving it earlier would make that side
    // effect observable on a path that previously short-circuited.
    // ClassRef constructors are non-pointer values with an own prototype.
    if extract_pointer(target.to_bits()) as usize == 0
        && crate::object::class_ref_id(target).is_none()
    {
        return None;
    }
    // #6943: `key_to_rust_string` runs the GC-capable `js_string_coerce`, and
    // `obj_ptr` is BOTH a dereferenced heap address (the typed-array probe) and
    // the raw ADDRESS KEY of the `ACCESSOR_DESCRIPTORS` /
    // `PROPERTY_DESCRIPTORS` side tables. A stale one does not crash: it
    // silently misses, so a non-writable own property or a setter-less accessor
    // reads back as "no descriptor" and the [[Set]] that should have been
    // rejected goes through. Root the receiver across the coercion and derive
    // the address afterwards. (Found in the same review pass as the `key` gap
    // below.)
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_nanbox_u64(target.to_bits());
    let key_name = key_to_rust_string(key)?;
    let target = f64::from_bits(target_handle.get_nanbox_u64());
    // Class constructors have an immutable own prototype even though their
    // ClassRef representation has no heap address or descriptor side table.
    if key_name == "prototype"
        && crate::object::class_ref_id(target).is_some()
        && crate::object::class_prototype_ref_id(target).is_none()
    {
        return Some(OwnSetDescriptor::Data { writable: false });
    }
    let obj_ptr = extract_pointer(target.to_bits()) as usize;
    if obj_ptr == 0 {
        return None;
    }
    // A typed array keeps its ordinary (non-index) own expando properties and
    // their descriptors in the typed-array side tables, which the generic
    // address-keyed lookups below skip (`object_has_descriptors` is
    // deliberately gated off for typed arrays). Consult that state directly so
    // a non-writable own data property / setter-less accessor rejects the write
    // (test262 TypedArray internals/Set key-is-not-numeric-index).
    if crate::typedarray_props::is_typed_array_owner(obj_ptr) {
        return match crate::typedarray_props::typed_array_own_set_descriptor(obj_ptr, &key_name) {
            Some(crate::typedarray_props::TypedArrayOwnSetDescriptor::Data { writable }) => {
                Some(OwnSetDescriptor::Data { writable })
            }
            Some(crate::typedarray_props::TypedArrayOwnSetDescriptor::Accessor { setter_bits }) => {
                Some(OwnSetDescriptor::Accessor { setter_bits })
            }
            None => None,
        };
    }
    // `ACCESSOR_DESCRIPTORS` / `PROPERTY_DESCRIPTORS` are keyed by raw address,
    // so a fresh object reusing a freed address would otherwise read back the
    // previous tenant's stale getter-only accessor / non-writable descriptor and
    // report this `obj.k = v` as read-only — falsely throwing "Cannot assign to
    // read only property" on a plain `{}` (Next.js app-page-turbo runtime's
    // `exports.Fragment = …`, reached here once a descriptor on Object.prototype
    // disables the plain-object [[Set]] fast path process-wide, #5054). Gate on
    // the per-object `OBJ_FLAG_HAS_DESCRIPTORS` flag — set reliably for every
    // descriptor installed on a `GC_TYPE_OBJECT`, and clear on a fresh
    // allocation. Closures don't carry the flag, so keep consulting the side
    // tables for them (their `name`/`length` + user `defineProperty` descriptors
    // live there).
    // A class function object's static accessor is an accessor property of
    // its own-property object (#11521: a getter-only one refuses the write).
    // Its own data properties live in the same object; anything else is not
    // own, and the walk continues at the class's [[Prototype]].
    if let Some(class_id) = crate::object::class_value::class_closure_id(obj_ptr) {
        if let Some((acc, _, _)) =
            crate::object::class_value::class_static_own_accessor(class_id, &key_name)
        {
            return Some(OwnSetDescriptor::Accessor {
                setter_bits: acc.set,
            });
        }
        if crate::object::class_value::class_static_get(class_id, &key_name).is_some() {
            let writable = crate::object::class_static_defined_attrs(class_id, &key_name)
                .is_none_or(|(writable, _, _)| writable);
            return Some(OwnSetDescriptor::Data { writable });
        }
        return None;
    }
    if crate::object::object_has_descriptors(obj_ptr)
        || unsafe {
            crate::object::key_attrs::attrs_live_in_keys(obj_ptr)
                && crate::object::key_attrs::object_summary(obj_ptr as *const crate::ObjectHeader)
                    != 0
        }
        || crate::closure::is_closure_ptr(obj_ptr)
    {
        if let Some(acc) = crate::object::get_accessor_descriptor(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Accessor {
                setter_bits: acc.set,
            });
        }
        if let Some(attrs) = crate::object::get_property_attrs(obj_ptr, &key_name) {
            return Some(OwnSetDescriptor::Data {
                writable: attrs.writable(),
            });
        }
    }
    if crate::closure::is_closure_ptr(obj_ptr) {
        if crate::object::has_own_helpers::closure_own_key_present(obj_ptr, &key_name) {
            // A function's lazily synthesized prototype is already an own
            // property. Preserve its attributes when the first operation is
            // an assignment, before any read materializes the default object.
            if key_name == "prototype" && crate::object::function_would_have_own_prototype(target) {
                crate::object::set_builtin_property_attrs(
                    obj_ptr,
                    key_name.clone(),
                    crate::object::PropertyAttrs::new(true, false, false),
                );
            }
            return Some(OwnSetDescriptor::Data {
                writable: !matches!(key_name.as_str(), "name" | "length"),
            });
        }
        return None;
    }
    if crate::object::obj_value_has_own_key(target, key) {
        return Some(OwnSetDescriptor::Data { writable: true });
    }
    None
}

fn prototype_of_for_set(value: f64) -> Option<f64> {
    if !reflect_value_is_object(value) {
        return None;
    }
    // A tagged Proxy receiver must dispatch before an ObjectHeader read.
    // Its own target/handler fields provide the prototype operation.
    if lookup(value).is_some() {
        let proto = reflect_misc::proxy_get_prototype_of_impl(value);
        let proto_bits = proto.to_bits();
        return if proto_bits == TAG_NULL
            || proto_bits == TAG_UNDEFINED
            || proto_bits == value.to_bits()
        {
            None
        } else {
            Some(proto)
        };
    }
    let bits = value.to_bits();
    if (bits >> 48) == (POINTER_TAG >> 48) {
        let raw = (bits & POINTER_MASK) as usize;
        // #7531: `lookup(value)` above only recognizes an already-registered
        // revocable-Proxy id; a fetch/zlib/common-registry handle id (or a
        // Proxy id from a DIFFERENT realm/registry) reaches here instead.
        // The old floor (`GC_HEADER_SIZE + 0x1000`) sits below every handle
        // band, and the local name `raw` made it invisible to
        // `scripts/addr_class_inventory.py`'s `handle-floor` regex. Below,
        // a rejected `object_static_prototype` lookup falls through to
        // `is_valid_obj_ptr(obj)` -- a magnitude-only check whose own floor
        // is 0x1000 -- followed by an unconditional `(*obj).class_id` read,
        // so an admitted handle id reached that deref.
        // A class function object is a closure, not an ObjectHeader: its
        // [[Prototype]] is the class's (the parent class for `extends`).
        if let Some(class_id) = crate::object::class_value::class_closure_id(raw) {
            let proto = crate::object::class_value::class_prototype_addr(class_id);
            return (proto != 0 && proto != raw)
                .then(|| f64::from_bits(POINTER_TAG | proto as u64));
        }
        if crate::value::addr_class::is_above_handle_band(raw) {
            if let Some(proto_bits) = crate::object::prototype_chain::object_static_prototype(raw) {
                if proto_bits == TAG_NULL || proto_bits == TAG_UNDEFINED || proto_bits == bits {
                    return None;
                }
                return Some(f64::from_bits(proto_bits));
            }
            let obj = raw as *const crate::ObjectHeader;
            if crate::object::is_valid_obj_ptr(obj as *const u8) {
                unsafe {
                    let class_id = (*obj).class_id;
                    if class_id != 0 {
                        let proto = crate::object::class_prototype_object(class_id);
                        if !proto.is_null() && proto as usize != raw {
                            return Some(crate::value::js_nanbox_pointer(proto as i64));
                        }
                    }
                }
            }
        }
    }
    let proto = crate::object::js_object_get_prototype_of(value);
    let bits = proto.to_bits();
    if bits == TAG_NULL || bits == TAG_UNDEFINED || bits == value.to_bits() {
        None
    } else {
        Some(proto)
    }
}

fn reflect_target_get_prototype_of(value: f64) -> f64 {
    prototype_of_for_set(value).unwrap_or_else(|| crate::object::js_object_get_prototype_of(value))
}

fn call_setter_with_receiver(setter_bits: u64, receiver: f64, value: f64) -> bool {
    if setter_bits == 0 {
        return false;
    }
    let this_scope = crate::gc::RuntimeHandleScope::new(); // #9445
    let receiver = this_scope.root_nanbox_f64(receiver);
    let value = this_scope.root_nanbox_f64(value);
    let rebound = crate::closure::clone_closure_rebind_this(setter_bits, receiver.get_nanbox_f64());
    let closure = closure_from(f64::from_bits(rebound));
    if closure.is_null() {
        return false;
    }
    let _ = crate::closure::js_closure_call1(
        closure,
        crate::closure::JsThis::from_f64(receiver.get_nanbox_f64()),
        value.get_nanbox_f64(),
    );
    true
}

/// Ordinary named-property Set for the BufferHeader-backed objects that are
/// not integer-indexed exotics (ArrayBuffer, SharedArrayBuffer, DataView).
///
/// These values have no ObjectHeader/GcHeader. The generic Set tail eventually
/// asks whether the receiver is a GC heap object before CreateDataProperty;
/// a process-global SharedArrayBuffer can therefore be rejected even though
/// the buffer registries authoritatively classify it as an Object. Walk the
/// descriptors normally, but materialize the final own data property in the
/// buffer expando table instead of the ObjectHeader store.
fn set_nonindexed_buffer_named_self(target: f64, key: f64, value: f64) -> Option<bool> {
    if unsafe { crate::symbol::js_is_symbol(key) } != 0 {
        return None;
    }
    let raw = raw_ptr_from_value(target)?;
    if !crate::buffer::is_non_indexed_buffer_view(raw) {
        return None;
    }
    let name = key_to_rust_string(key)?;

    // Existing own accessor/data descriptors take the same precedence as in
    // OrdinarySetWithOwnDescriptor. Buffer expandos live in side tables, so
    // inspect those tables without treating the BufferHeader as ObjectHeader.
    if let Some(accessor) = crate::object::get_accessor_descriptor(raw, &name) {
        return Some(call_setter_with_receiver(accessor.set, target, value));
    }
    if crate::buffer::buffer_has_own_prop(raw, &name) {
        if crate::object::get_property_attrs(raw, &name).is_some_and(|attrs| !attrs.writable()) {
            return Some(false);
        }
        crate::buffer::buffer_set_own_prop(raw, &name, value);
        return Some(true);
    }

    // A descriptor on the prototype can reject the write or invoke a setter.
    // A writable inherited data descriptor (including the standard
    // `.constructor`) creates an own property on the original receiver.
    let mut current = prototype_of_for_set(target);
    for _ in 0..64 {
        let Some(proto) = current else {
            break;
        };
        if let Some(desc) = own_set_descriptor(proto, key) {
            match desc {
                OwnSetDescriptor::Data { writable: false } => return Some(false),
                OwnSetDescriptor::Data { writable: true } => break,
                OwnSetDescriptor::Accessor { setter_bits } => {
                    return Some(call_setter_with_receiver(setter_bits, target, value));
                }
            }
        }
        current = prototype_of_for_set(proto);
    }

    crate::buffer::buffer_set_own_prop(raw, &name, value);
    Some(true)
}

/// #5129: build a fresh data property descriptor
/// `{ value, writable: true, enumerable: true, configurable: true }`
/// (the CreateDataProperty shape) for defining a property on a Proxy receiver
/// via its `[[DefineOwnProperty]]`.
unsafe fn build_create_data_descriptor(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_root = scope.root_nanbox_f64(value);
    let desc = crate::object::js_object_alloc(0, 4);
    let desc_handle = scope.root_raw_mut_ptr(desc);
    for (name, field) in [
        (b"value".as_slice(), value_root.get_nanbox_f64()),
        (b"writable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"enumerable".as_slice(), f64::from_bits(TAG_TRUE)),
        (b"configurable".as_slice(), f64::from_bits(TAG_TRUE)),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        crate::object::js_object_set_field_by_name(
            desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
            key,
            field,
        );
    }
    f64::from_bits(
        POINTER_TAG
            | ((desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64) & POINTER_MASK),
    )
}

/// Define a writable, enumerable, configurable own data property using the
/// receiver's `[[DefineOwnProperty]]`. This is the operation used by public
/// class fields: it bypasses inherited setters on ordinary objects and still
/// drives a Proxy's `defineProperty` trap.
pub(crate) fn create_data_property(receiver: f64, key: f64, value: f64) -> bool {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let key = scope.root_nanbox_f64(key);
    let value = scope.root_nanbox_f64(value);
    let descriptor = unsafe { build_create_data_descriptor(value.get_nanbox_f64()) };
    let descriptor = scope.root_nanbox_f64(descriptor);
    crate::value::js_is_truthy(js_reflect_define_property(
        receiver.get_nanbox_f64(),
        key.get_nanbox_f64(),
        descriptor.get_nanbox_f64(),
    )) != 0
}

/// #5129: build a `{ value }`-only property descriptor — the `valueDesc` of
/// OrdinarySetWithOwnDescriptor step 2.d.iii, used to update an existing
/// writable data property on a Proxy receiver without disturbing its other
/// attributes (`writable`/`enumerable`/`configurable`).
unsafe fn build_value_only_descriptor(value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_root = scope.root_nanbox_f64(value);
    let desc = crate::object::js_object_alloc(0, 1);
    let desc_handle = scope.root_raw_mut_ptr(desc);
    let key = crate::string::js_string_from_bytes(b"value".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(
        desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>(),
        key,
        value_root.get_nanbox_f64(),
    );
    f64::from_bits(
        POINTER_TAG
            | ((desc_handle.get_raw_mut_ptr::<crate::ObjectHeader>() as u64) & POINTER_MASK),
    )
}

fn create_or_update_receiver_property(receiver: f64, key: f64, value: f64) -> bool {
    if !reflect_value_is_object(receiver) {
        return false;
    }
    // #5129: a Proxy receiver — e.g. a `set` trap forwarding
    // `Reflect.set(target, key, value, proxy)` (the 4-arg form) — must route
    // through the proxy's `[[DefineOwnProperty]]` (its `defineProperty` trap,
    // or, absent a trap, a define on the proxy's target), NOT an ordinary data
    // store. This is OrdinarySetWithOwnDescriptor's tail
    // `CreateDataProperty(Receiver, P, V)`. Treating the proxy id as a heap
    // object (the `target_set` fall-through below) segfaulted; re-invoking the
    // `set` trap would have recursed infinitely.
    if lookup(receiver).is_some() {
        // OrdinarySetWithOwnDescriptor steps 2.c–2.e for a Proxy receiver. We
        // must mirror the ordinary algorithm's receiver-own-descriptor checks,
        // not jump straight to CreateDataProperty:
        //
        //   2.c existingDescriptor = Receiver.[[GetOwnProperty]](P)
        //   2.d if it exists:
        //       i.   accessor descriptor      → return false
        //       ii.  non-writable data        → return false
        //       iii. else redefine `{ value }` only (preserve other attrs)
        //   2.e else CreateDataProperty(Receiver, P, V)
        //
        // `[[GetOwnProperty]]` fires the proxy's getOwnPropertyDescriptor trap
        // (trap-less: reads its target) and returns a completed plain
        // descriptor object, or `undefined` when absent. A throwing/invariant-
        // violating trap unwinds via `js_throw` and never returns here.
        let existing = js_reflect_get_own_property_descriptor(receiver, key);
        let desc = if reflect_value_is_object(existing) {
            let is_accessor = unsafe {
                reflect::descriptor_field_present(existing, b"get")
                    || reflect::descriptor_field_present(existing, b"set")
            };
            // A completed data descriptor always carries `writable`; treat a
            // missing flag as non-writable (reject) to stay on the safe side.
            let writable = unsafe { reflect::descriptor_bool_field(existing, b"writable") };
            if is_accessor || writable != Some(true) {
                return false;
            }
            unsafe { build_value_only_descriptor(value) }
        } else {
            unsafe { build_create_data_descriptor(value) }
        };
        return crate::value::js_is_truthy(js_reflect_define_property(receiver, key, desc)) != 0;
    }
    if let Some(desc) = own_set_descriptor(receiver, key) {
        match desc {
            OwnSetDescriptor::Data { writable } => {
                if !writable {
                    return false;
                }
            }
            OwnSetDescriptor::Accessor { .. } => {
                // OrdinarySetWithOwnDescriptor step 2.d.i: reaching here means
                // the source (target own) descriptor was a data property (or
                // absent -> CreateDataProperty). A RECEIVER own accessor makes
                // the algorithm return false WITHOUT invoking the setter -- the
                // setter only fires when it is the descriptor found by the
                // OrdinarySet walk itself (the Accessor arm in
                // ordinary_set_with_receiver), never through this
                // CreateDataProperty-on-receiver tail.
                return false;
            }
        }
    } else if crate::object::obj_value_no_extend(receiver) {
        return false;
    } else if unsafe { crate::object::layout_desc::family_create(receiver, key, value) } {
        // Case 2c: a family object's late key, or a key outside its layout.
        return true;
    } else {
        // OrdinarySetWithOwnDescriptor creates an OWN data property here.
        // Re-entering [[Set]] would walk the receiver's prototypes again and
        // recurse forever when that chain contains a trapless Proxy.
        let scope = crate::gc::RuntimeHandleScope::new();
        let receiver = scope.root_nanbox_f64(receiver);
        let key = scope.root_nanbox_f64(key);
        let descriptor = crate::object::object_ops::DescView::create_data(&scope, value);
        return unsafe {
            crate::object::object_ops::define_own_property_decoded(
                &scope,
                &receiver,
                &key,
                &descriptor,
            )
        };
    }
    target_set(receiver, key, value);
    true
}

fn ordinary_set_with_receiver(target: f64, key: f64, value: f64, receiver: f64) -> bool {
    if let Some(ok) = set_handle_property(target, key, value) {
        return ok;
    }

    // #5054 fast path: the spec walk below probes own_set_descriptor on the
    // target, which ends in a LINEAR keys_array scan — so every dynamic
    // `obj[key] = v` was O(own-key-count) and building a wide dynamic object
    // quadratic (10k props ~ 12s). When nothing the walk models can apply,
    // the write reduces to the ordinary data-property store:
    //   - target written as itself (receiver bits identical),
    //   - plain GC_TYPE_OBJECT with class_id 0 (no class setter machinery),
    //   - no descriptor ever installed on THIS object
    //     (OBJ_FLAG_HAS_DESCRIPTORS) and not frozen/sealed/non-extensible,
    //   - no recorded setPrototypeOf target (prototype chain is exactly
    //     Object.prototype) and no descriptor on Object.prototype,
    //   - string key.
    let target_top16 = target.to_bits() >> 48;
    if target.to_bits() == receiver.to_bits()
        // POINTER_TAG'd heap object, or a module-level slot's raw I64 pointer
        // (top 16 bits zero).
        && (target_top16 == 0x7FFD || target_top16 == 0)
        && unsafe { crate::symbol::js_is_symbol(key) } == 0
    {
        let addr = extract_pointer(target.to_bits()) as usize;
        // Typed arrays must be excluded before the header probe: small TAs
        // are plain-alloc'd without a GcHeader.
        if !crate::typedarray_props::is_typed_array_owner(addr)
            && crate::object::exotic_expando::exotic_expando_kind_of_value(target).is_none()
            && !crate::closure::is_closure_ptr(addr)
        {
            unsafe {
                if let Some(header) = crate::value::addr_class::try_read_gc_header(addr) {
                    const SLOW_FLAGS: u16 = crate::gc::OBJ_FLAG_FROZEN
                        | crate::gc::OBJ_FLAG_SEALED
                        | crate::gc::OBJ_FLAG_NO_EXTEND;
                    // #10287: an own descriptor is vetted per KEY (the summary
                    // proves absence outright), so one `defineProperty` no
                    // longer sends every later store on this receiver down the
                    // full walk. Read before the coercion below: it reads key
                    // bytes only and cannot allocate.
                    if header.obj_type == crate::gc::GC_TYPE_OBJECT
                        && header._reserved & SLOW_FLAGS == 0
                        && crate::object::own_descriptors_skip_key(addr, key)
                    {
                        let class_id = (*(addr as *const crate::ObjectHeader)).class_id;
                        // #6943: BOTH arms below reach a GC-capable
                        // `js_string_coerce` on `key` — the class arm calls it
                        // directly for the store-plan key, and the plain-object
                        // arm reaches it through
                        // `object_proto_may_intercept_key` ->
                        // `obj_value_has_own_key`. The coercion is inert only
                        // for an already-heap `STRING_TAG` key; an SSO short
                        // key materializes onto the heap, a numeric key builds
                        // its stringification, and an object key runs a user
                        // `toString` / `valueOf`. Any of those can trigger a GC
                        // that **evacuates**.
                        //
                        // The rule is: when the coercion is NOT inert, every
                        // operand that outlives it must be rooted and re-read
                        // afterwards. That is four of them, not three —
                        // `addr` (the receiver, dereferenced for `plan_eligible`
                        // and passed to `class_instance_set_may_intercept`),
                        // `target` and the `value` about to be written INTO it,
                        // and **`key` itself**, which is re-used at the
                        // interception check and again at `target_set`. An
                        // object key is a `POINTER_TAG` heap value and is
                        // exactly the shape that runs the user JS which can
                        // evacuate it. (`key` was missed in the first pass of
                        // this fix; caught in review.)
                        //
                        // Only an already-heap `STRING_TAG` key is inert — it
                        // is handed straight back with no allocation — so that
                        // one shape takes no scope. Every other shape does: an
                        // SSO short key materializes onto the heap, a numeric
                        // key builds its stringification, and an object key
                        // runs a user `toString` / `valueOf`.
                        let scope = (!crate::builtins::string_coerce_is_inert(key))
                            .then(crate::gc::RuntimeHandleScope::new);
                        let roots = scope.as_ref().map(|s| {
                            (
                                s.root_nanbox_u64(target.to_bits()),
                                s.root_nanbox_f64(value),
                                s.root_raw_mut_ptr(addr as *mut u8),
                                s.root_nanbox_f64(key),
                            )
                        });
                        // Re-read an operand through its handle (or pass the
                        // original through untouched on the inert path).
                        let cur_target = || match &roots {
                            Some((t, ..)) => f64::from_bits(t.get_nanbox_u64()),
                            None => target,
                        };
                        let cur_value = || match &roots {
                            Some((_, v, ..)) => v.get_nanbox_f64(),
                            None => value,
                        };
                        let cur_addr = || match &roots {
                            Some((_, _, a, _)) => a.get_raw_mut_ptr::<u8>() as usize,
                            None => addr,
                        };
                        let cur_key = || match &roots {
                            Some((_, _, _, k)) => k.get_nanbox_f64(),
                            None => key,
                        };
                        let fast_safe = if class_id == 0 {
                            // Plain object: prototype is exactly Object.prototype, and
                            // Object.prototype doesn't intercept this key (per-key, not
                            // the coarse process-wide descriptor flag — that made wide
                            // builds O(n²)). #10287: a RECORDED prototype (a
                            // function-constructed receiver, `Object.create`,
                            // `setPrototypeOf`) is walked per key rather than rejected.
                            // A born-null receiver whose shape proves it ordinary
                            // with no chain has nothing to intercept the key.
                            use crate::object::shapes::store_kind;
                            let obj = cur_addr() as *const crate::ObjectHeader;
                            if header._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
                                && store_kind::born_null_shape_proven(obj, header._reserved)
                            {
                                true
                            } else {
                                match crate::object::prototype_chain::object_static_prototype(
                                    cur_addr(),
                                ) {
                                    None => {
                                        !crate::object::object_proto_may_intercept_key(cur_key())
                                    }
                                    Some(_) => {
                                        !crate::object::plain_custom_prototype_may_intercept(
                                            cur_addr(),
                                            cur_key(),
                                        )
                                    }
                                }
                            }
                        } else {
                            // `DisposableStack#disposed` is a getter-only
                            // builtin accessor on a reserved native prototype.
                            // Those class ids are intentionally absent from the
                            // JS class-vtable registry, so the shared
                            // class-interception plan cannot discover it.
                            // Keep an own descriptor on the instance eligible
                            // for the normal walk; otherwise report the
                            // inherited accessor's rejected [[Set]] here
                            // (silent in sloppy PutValue, TypeError in strict).
                            let inherited_disposed_readonly = matches!(
                                class_id,
                                crate::disposable::CLASS_ID_DISPOSABLE_STACK
                                    | crate::disposable::CLASS_ID_ASYNC_DISPOSABLE_STACK
                            ) && property_key_to_rust_string(key)
                                .as_deref()
                                == Some("disposed")
                                // #6943: `property_key_to_rust_string` runs
                                // `ToPropertyKey` + `js_string_coerce`, so both
                                // operands must be re-read before this probe.
                                && own_set_descriptor(cur_target(), cur_key()).is_none();
                            if inherited_disposed_readonly {
                                return false;
                            }
                            // Class instance: the `class_id == 0` guard previously sent
                            // EVERY wide class-instance build down the O(own-key) slow
                            // walk (O(n²)). Safe to fast-path when no inherited accessor /
                            // non-writable anywhere in the prototype chain could intercept
                            // this key.
                            //
                            // This receiver-based [[Set]] is the dominant real-code store
                            // path (codegen emits js_put_value_set for many `obj.f = v`),
                            // and it re-ran the full O(chain) interception walk on every
                            // call. Consult the SHARED store-plan cache first: the verdict
                            // is the identical `!class_instance_set_may_intercept(class_id,
                            // key)` that `js_object_set_field_by_name` already memoizes, so
                            // a hit on either path serves the other. Only trust a cached
                            // class-chain verdict for instances whose chain matches the
                            // class chain — SLOW_FLAGS above already excluded
                            // frozen/sealed/descriptor bits; add the per-instance
                            // divergence flags (setPrototypeOf override / null proto).
                            let key_ptr = crate::builtins::js_string_coerce(cur_key())
                                as *const crate::StringHeader;
                            // #6943: re-read the receiver AND the key through
                            // their handles — everything below uses both.
                            let addr = cur_addr();
                            let key = cur_key();
                            let interned = crate::object::interned_key_ptr(key_ptr);
                            // #6595: a per-evaluation CLASS OBJECT (what a
                            // capture-carrying class materializes as,
                            // `ShapeObjectKind::Class`) shares its
                            // template cid with its instances, and its own-data
                            // writes must reach the #6530
                            // `mirror_class_object_static_write` hook in
                            // `js_object_set_field_by_name`. Recording a plan
                            // here armed the mirror-free fast lane for the very
                            // store being vetted, so from the second same-shaped
                            // class on (shape-transition cache hit) post-class
                            // statics like bundled zod's `ZodX.create` vanished
                            // from ClassRef static dispatch. Class objects
                            // neither record nor honor store plans.
                            let plan_eligible = class_id != crate::object::NATIVE_MODULE_CLASS_ID
                                // #8113: this asks for ORDINARY specifically —
                                // it must stay FALSE for a class object or
                                // #6595 reopens. `object_is_regular` is exactly
                                // `descriptor.object_kind == Ordinary` since
                                // #8086, so it is the same predicate the
                                // deleted `object_type == OBJECT_TYPE_REGULAR`
                                // word expressed, not the weaker
                                // "is an ObjectHeader" test.
                                && crate::object::object_is_regular(
                                    addr as *const crate::ObjectHeader,
                                )
                                // A born-null receiver's plan is keyed on a
                                // shape that proves it ordinary with no chain.
                                && crate::object::shapes::store_kind::born_null_admits_lane(
                                    addr as *const crate::ObjectHeader,
                                    header._reserved,
                                )
                                && interned != 0;
                            let verdict = if plan_eligible
                                && crate::object::prop_plan::store_plan_check(
                                    crate::object::shapes::object_shape_stamp(
                                        addr as *const crate::ObjectHeader,
                                    ),
                                    interned,
                                ) {
                                true
                            } else {
                                let clear = !crate::object::class_instance_set_may_intercept(
                                    addr, class_id, key,
                                );
                                if clear && plan_eligible {
                                    crate::object::prop_plan::store_plan_record(
                                        crate::object::shapes::object_shape_stamp(
                                            cur_addr() as *const crate::ObjectHeader
                                        ),
                                        interned,
                                    );
                                }
                                clear
                            };
                            verdict
                        };
                        // #6943: the store itself takes the refreshed receiver,
                        // KEY and payload — all three were rooted across
                        // whichever coercion the arm above performed.
                        if fast_safe {
                            target_set(cur_target(), cur_key(), cur_value());
                            return true;
                        }
                    }
                }
            }
        }
    }

    // CommonJS native-module namespaces are MUTABLE in Node — monkey-patching
    // like Next.js's `require('node:timers').setImmediate = patched` must
    // store the override (read back through the namespace vtable's
    // `get_own_field`) rather than reporting the built-in member
    // non-writable and throwing under strict mode.
    {
        let jv = crate::value::JSValue::from_bits(target.to_bits());
        if jv.is_pointer() {
            let obj = extract_pointer(target.to_bits()) as *const crate::object::ObjectHeader;
            if !obj.is_null() && unsafe { (*obj).class_id } == crate::object::NATIVE_MODULE_CLASS_ID
            {
                let module_name = unsafe { crate::object::get_module_name_from_namespace(target) };
                if let (false, Some(prop)) =
                    (module_name.is_empty(), property_key_to_rust_string(key))
                {
                    if prop != "__module__" {
                        if module_name == "buffer.Buffer" && prop == "poolSize" {
                            crate::object::set_buffer_pool_size(value);
                        } else {
                            crate::object::native_namespace_prop_override_store(
                                &module_name,
                                &prop,
                                value,
                            );
                        }
                        return true;
                    }
                }
            }
        }
    }

    set_interceptor(target, key, value, receiver)
        .unwrap_or_else(|| create_or_update_receiver_property(receiver, key, value))
}

/// #11201: `[[Set]]` reaching a class-backed link — a declared or
/// per-evaluation class prototype, or a class instance used as a prototype
/// (`Object.create(new C())`) — on the chain of a receiver with class id 0.
/// The link's accessors live in the class vtable, not in the descriptor tables
/// `own_set_descriptor` reads, so without this the walk ran past an inherited
/// `set x` and created an own data property instead of calling it.
///
/// A receiver WITH a class id never takes this arm: its write reaches
/// `js_object_set_field_by_name`, whose vtable walk is keyed by that id. An
/// own property of the link has already been answered by `own_set_descriptor`.
/// `Some(true)`: the setter ran; `Some(false)`: getter-only, the write is
/// rejected; `None`: no accessor for `key`, continue the walk.
fn class_link_accessor_set(current: f64, key: f64, value: f64, receiver: f64) -> Option<bool> {
    let link = extract_pointer(current.to_bits()) as *const crate::ObjectHeader;
    let recv = extract_pointer(receiver.to_bits()) as *const crate::ObjectHeader;
    if link.is_null() || crate::object::js_object_get_class_id(recv) != 0 {
        return None;
    }
    // A class function object is a closure (no ObjectHeader class id): its
    // static accessors are own properties `own_set_descriptor` reports.
    if crate::object::class_value::class_closure_id(link as usize).is_some() {
        return None;
    }
    let class_id = crate::object::js_object_get_class_id(link);
    if class_id == 0
        || class_id == crate::object::NATIVE_MODULE_CLASS_ID
        || crate::object::is_class_object_ptr(link.cast())
    {
        return None;
    }
    class_super_accessor_set(class_id, key, value, receiver)
}

fn class_super_accessor_set(
    parent_class_id: u32,
    key: f64,
    value: f64,
    receiver: f64,
) -> Option<bool> {
    let key_name = property_key_to_rust_string(key)?;
    // Charter step 3: the accessor is a property of the parent's prototype
    // chain. `Some(false)`: getter-only, the write is refused.
    unsafe { crate::object::class_chain_setter_apply(parent_class_id, &key_name, receiver, value) }
}

fn receiver_super_parent_class_id(receiver: f64) -> Option<u32> {
    let obj = extract_pointer(receiver.to_bits()) as *const crate::ObjectHeader;
    if obj.is_null() {
        return None;
    }
    let class_id = unsafe { (*obj).class_id };
    if class_id == 0 {
        return None;
    }
    crate::object::get_parent_class_id(class_id)
}

fn normalize_accessor_receiver(receiver: f64) -> f64 {
    let bits = receiver.to_bits();
    if bits != 0 && (bits >> 48) == 0 {
        crate::value::js_nanbox_pointer(bits as i64)
    } else if receiver.is_finite() && receiver > 65_536.0 && receiver.fract() == 0.0 {
        crate::value::js_nanbox_pointer(receiver as i64)
    } else {
        receiver
    }
}

/// `super[key] = value` for class methods. The property lookup starts at the
/// parent prototype, but writes use the current `this` as Receiver.
#[no_mangle]
pub extern "C" fn js_super_put_value_set(
    parent_class_id: u32,
    key: f64,
    value: f64,
    receiver: f64,
    strict: i32,
) -> f64 {
    let receiver = normalize_accessor_receiver(receiver);
    // Static-context super uses the parent CONSTRUCTOR as the lookup target,
    // while retaining the child constructor as Receiver. A statically-known
    // class parent is represented by a ClassRef; a function-valued parent is
    // the value captured at class-definition time. The previous instance-only
    // path looked at `Parent.prototype` and made valid static writes fail.
    if let Some(child_id) = crate::object::class_ref_id(receiver) {
        // A builtin parent id (`extends Error`) never gets a class function
        // object; its constructor is the class's dynamic parent value.
        let target =
            if parent_class_id != 0 && crate::object::is_class_id_registered(parent_class_id) {
                crate::object::class_value::class_value(parent_class_id)
            } else {
                crate::object::js_get_dynamic_parent_value(child_id)
            };
        let tv = crate::value::JSValue::from_bits(target.to_bits());
        if !tv.is_undefined() && !tv.is_null() {
            return js_put_value_set(target, key, value, receiver, strict);
        }
        // A base class's constructor inherits from Function.prototype. Perry
        // does not materialize that object for this path; when lookup misses,
        // OrdinarySet creates the own property on Receiver.
        return js_put_value_set(receiver, key, value, receiver, strict);
    }
    if parent_class_id == 0 && crate::object::is_class_object_value(receiver) {
        let obj = crate::value::JSValue::from_bits(receiver.to_bits())
            .as_pointer::<crate::ObjectHeader>();
        let child_id = if obj.is_null() {
            0
        } else {
            crate::object::js_object_get_class_id(obj)
        };
        let dynamic_parent = crate::object::js_get_dynamic_parent_value(child_id);
        if crate::value::JSValue::from_bits(dynamic_parent.to_bits()).is_undefined() {
            return js_put_value_set(receiver, key, value, receiver, strict);
        }
        return js_put_value_set(dynamic_parent, key, value, receiver, strict);
    }
    let receiver_parent_class_id = receiver_super_parent_class_id(receiver);
    if let Some(ok) =
        class_super_accessor_set(parent_class_id, key, value, receiver).or_else(|| {
            receiver_parent_class_id
                .filter(|cid| *cid != parent_class_id)
                .and_then(|cid| class_super_accessor_set(cid, key, value, receiver))
        })
    {
        if !ok && strict != 0 {
            let key_name = key_to_rust_string(key).unwrap_or_else(|| "property".to_string());
            crate::error::throw_immutable_write(0, &key_name);
        }
        return value;
    }

    let effective_parent_class_id = if parent_class_id != 0 {
        parent_class_id
    } else {
        receiver_parent_class_id.unwrap_or(0)
    };
    let proto = crate::object::class_prototype_object(effective_parent_class_id);
    if !proto.is_null() {
        let target = crate::value::js_nanbox_pointer(proto as i64);
        return js_put_value_set(target, key, value, receiver, strict);
    }

    // No resolvable parent-class prototype — `super` is `Object.prototype`
    // (e.g. `class A {}` with no `extends`). Per spec `super.x = v` performs
    // the home object's prototype `[[Set]]` with `this` as the receiver, which
    // for a missing key + no inherited setter creates an own data property on
    // the receiver. Do that ordinary set instead of throwing. (Test262
    // syntax/class-body-method-definition-super-property.)
    js_put_value_set(receiver, key, value, receiver, strict)
}

/// `Proxy.revocable(target, handler)` — returns an ordinary object
/// `{ proxy, revoke }` where `proxy` is a fresh revocable Proxy and `revoke`
/// is a callable, idempotent function that revokes only that proxy. (#2846)
///
/// Unlike the destructuring fast-path in `stmt.rs`, this builds a real heap
/// object so `typeof rec.revoke === "function"`, `rec.proxy.a` forwards, and
/// the revoke function can be stored/aliased and still work.
#[no_mangle]
pub extern "C" fn js_proxy_revocable(target: f64, handler: f64) -> f64 {
    // Reuse `js_proxy_new` so the same object-argument validation applies.
    let proxy = js_proxy_new(target, handler);

    // Build the revoke closure capturing the proxy value.
    let revoke_closure = crate::closure::js_closure_alloc(
        crate::fn_info!(reflect_misc::proxy_revoke_trampoline, 0; with_declared(0)),
        1,
    );
    crate::closure::js_closure_set_capture_f64(revoke_closure, 0, proxy);
    let revoke_boxed = f64::from_bits(POINTER_TAG | ((revoke_closure as u64) & POINTER_MASK));

    // Build the `{ proxy, revoke }` record. Root everything across the
    // intermediate allocations so a GC during key/string allocation can't
    // strand the proxy/revoke values.
    let scope = crate::gc::RuntimeHandleScope::new();
    let proxy_root = scope.root_nanbox_f64(proxy);
    let revoke_root = scope.root_nanbox_f64(revoke_boxed);

    let obj = crate::object::js_object_alloc(0, 2);
    let obj_handle = scope.root_raw_mut_ptr(obj);
    let keys = crate::array::js_array_alloc(0);
    let obj = obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
    crate::object::js_object_set_keys(obj, keys);

    let proxy_key = crate::string::js_string_from_bytes(b"proxy".as_ptr(), 5);
    crate::object::js_object_set_field_by_name(obj, proxy_key, proxy_root.get_nanbox_f64());
    let obj = obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
    let revoke_key = crate::string::js_string_from_bytes(b"revoke".as_ptr(), 6);
    crate::object::js_object_set_field_by_name(obj, revoke_key, revoke_root.get_nanbox_f64());

    let obj = obj_handle.get_raw_mut_ptr::<crate::ObjectHeader>();
    f64::from_bits(POINTER_TAG | ((obj as u64) & POINTER_MASK))
}

// #2846: retention anchor for `Proxy.revocable` (codegen-only callsite).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_PROXY_REVOCABLE: extern "C" fn(f64, f64) -> f64 = js_proxy_revocable;

// #2762: retention anchors for the Reflect-specific extensibility entry points.
// These `#[no_mangle]` fns are emitted only by codegen (no Rust caller in the
// crate graph), so the auto-optimize whole-program LLVM bitcode rebuild would
// otherwise internalize and dead-strip them. See node_stream_keepalive.rs.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_IS_EXTENSIBLE: extern "C" fn(f64) -> f64 = js_reflect_is_extensible;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_PREVENT_EXTENSIONS: extern "C" fn(f64) -> f64 = js_reflect_prevent_extensions;

// #2761: retention anchor for `Reflect.setPrototypeOf` (codegen-only callsite).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_SET_PROTOTYPE_OF: extern "C" fn(f64, f64) -> f64 = js_reflect_set_prototype_of;

// #2763/#2764/#2766/#2767: retention anchors for the Reflect entry points
// whose only callsites are codegen-emitted. `js_reflect_get` gained a third
// `receiver` arg (#2766) and must keep its new signature retained.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_GET: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_get;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_GET_OWN_PROPERTY_DESCRIPTOR: extern "C" fn(f64, f64) -> f64 =
    js_reflect_get_own_property_descriptor;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_HAS: extern "C" fn(f64, f64) -> f64 = js_reflect_has;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_OWN_KEYS: extern "C" fn(f64) -> f64 = js_reflect_own_keys;
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_REFLECT_APPLY: extern "C" fn(f64, f64, f64) -> f64 = js_reflect_apply;

#[cfg(test)]
pub(crate) fn test_proxy_slot_is_live(proxy_boxed: f64) -> bool {
    // A collected cell is no longer an honest pointer: ask the allocator's
    // object-start record, not the stale header bytes.
    let addr = (proxy_boxed.to_bits() & POINTER_MASK) as usize;
    unsafe {
        crate::value::addr_class::try_read_tracked_arena_header_of_type(
            addr,
            crate::gc::GC_TYPE_PROXY,
        )
    }
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj_value() -> f64 {
        let obj = crate::object::js_object_alloc(0, 0);
        f64::from_bits(POINTER_TAG | ((obj as u64) & POINTER_MASK))
    }

    #[test]
    fn revoke_detaches_target_and_handler_slots() {
        let target = obj_value();
        let handler = obj_value();
        let proxy = js_proxy_new(target, handler);

        assert_eq!(js_proxy_is_revoked(proxy), 0);
        assert_eq!(js_proxy_target(proxy).to_bits(), target.to_bits());
        assert_eq!(js_proxy_handler(proxy).to_bits(), handler.to_bits());

        js_proxy_revoke(proxy);
        assert_eq!(js_proxy_is_revoked(proxy), 1);

        let id = lookup(proxy).expect("revoked proxy stays a registered proxy");
        let (target_bits, handler_bits) = {
            let entry = entry(id).expect("live proxy cell");
            (entry.target.to_bits(), entry.handler.to_bits())
        };
        assert_eq!(target_bits, 0, "revoke must null [[ProxyTarget]]");
        assert_eq!(handler_bits, 0, "revoke must null [[ProxyHandler]]");

        // Detached slots are reported as `undefined`, never as raw 0.0.
        assert_eq!(js_proxy_target(proxy).to_bits(), TAG_UNDEFINED);
        assert_eq!(js_proxy_handler(proxy).to_bits(), TAG_UNDEFINED);

        // Idempotent.
        js_proxy_revoke(proxy);
        assert_eq!(js_proxy_is_revoked(proxy), 1);
    }

    /// `typeof` of a revoked proxy is unchanged (spec: [[Call]] presence is
    /// fixed at creation), so callability must survive target detachment —
    /// including through a nested proxy whose inner proxy gets revoked.
    #[test]
    fn revoked_proxy_keeps_creation_callability() {
        extern "C" fn dummy_fn(
            _closure: *const crate::closure::ClosureHeader,
            _this: crate::closure::JsThis,
        ) -> f64 {
            f64::from_bits(TAG_UNDEFINED)
        }
        let f = crate::closure::js_closure_alloc(crate::fn_info!(dummy_fn, 0), 0);
        let f_val = f64::from_bits(POINTER_TAG | ((f as u64) & POINTER_MASK));
        let handler = obj_value();

        let callable_proxy = js_proxy_new(f_val, handler);
        assert!(proxy_wraps_callable(callable_proxy));
        let nested = js_proxy_new(callable_proxy, handler);
        assert!(
            proxy_wraps_callable(nested),
            "nested proxy inherits the inner proxy's creation-time snapshot"
        );

        js_proxy_revoke(callable_proxy);
        assert!(
            proxy_wraps_callable(callable_proxy),
            "typeof of a revoked proxy must be unchanged despite the nulled target"
        );
        assert!(proxy_wraps_callable(nested));

        // And a plain-object proxy stays non-callable across revocation.
        let plain_proxy = js_proxy_new(obj_value(), obj_value());
        assert!(!proxy_wraps_callable(plain_proxy));
        js_proxy_revoke(plain_proxy);
        assert!(!proxy_wraps_callable(plain_proxy));
    }

    fn fnv1a(bytes: &[u8]) -> u64 {
        crate::codegen_abi::property_hash::hash(bytes)
    }

    fn boxed_object(obj: *mut crate::ObjectHeader) -> f64 {
        f64::from_bits(POINTER_TAG | (obj as u64 & POINTER_MASK))
    }

    fn boxed_interned_key(keys: *mut crate::ArrayHeader, slot: u32, name: &[u8]) -> f64 {
        let key = crate::array::js_array_get(keys, slot).as_string_ptr();
        let key = crate::string::js_string_intern(key, fnv1a(name));
        f64::from_bits(crate::value::STRING_TAG | (key as u64 & POINTER_MASK))
    }

    fn object_array_numeric_write_guard(array: f64, keys: &[f64], count: u32) -> u64 {
        assert!((1..=4).contains(&keys.len()));
        let mut padded = [0.0; 4];
        padded[..keys.len()].copy_from_slice(keys);
        put_value::js_object_array_numeric_write_guard(
            array,
            padded[0],
            padded[1],
            padded[2],
            padded[3],
            keys.len() as u32,
            count,
        )
    }

    fn object_array_numeric_write_range_guard(
        array: f64,
        keys: &[f64],
        start: u32,
        end: u32,
    ) -> u64 {
        assert!((1..=4).contains(&keys.len()));
        let mut padded = [0.0; 4];
        padded[..keys.len()].copy_from_slice(keys);
        put_value::js_object_array_numeric_write_range_guard(
            array,
            padded[0],
            padded[1],
            padded[2],
            padded[3],
            keys.len() as u32,
            start,
            end,
        )
    }

    /// #6809/#6812: the whole-loop preflight may only publish raw slot indexes
    /// when every array element has the same writable data layout. The
    /// generated clone performs no checks after this result, so heterogeneous
    /// shapes, holes, descriptor flags, class-id-zero objects, and forged or
    /// stale typed-layout state must all reject.
    #[test]
    fn object_array_numeric_write_guard_requires_complete_uniform_proof() {
        // This proof honours the process-wide class-field inline gate, and any
        // earlier test that ran `js_gc_init` (typed feedback is always on in
        // test builds) leaves that gate disabled. Establish the premise.

        let packed = b"a\0b\0c\0d\0";
        let keys = crate::object::js_build_class_keys_array(
            0x6809_01,
            4,
            packed.as_ptr(),
            packed.len() as u32,
            0,
        );
        let first = crate::object::js_object_alloc_class_inline_keys(0x6809_01, 0, 4, keys);
        let second = crate::object::js_object_alloc_class_inline_keys(0x6809_01, 0, 4, keys);
        let values = [boxed_object(first), boxed_object(second)];
        let array = crate::array::js_array_from_f64(values.as_ptr(), values.len() as u32);
        let array_box = boxed_object(array.cast());
        let a = boxed_interned_key(keys, 0, b"a");
        let b = boxed_interned_key(keys, 1, b"b");
        let c = boxed_interned_key(keys, 2, b"c");
        let d = boxed_interned_key(keys, 3, b"d");

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[c], 2),
            3,
            "one-field loops should publish one non-zero 16-bit lane"
        );
        let plain_a_ptr = crate::string::js_string_from_bytes(b"a".as_ptr(), 1);
        let plain_a_gc =
            unsafe { crate::value::addr_class::try_read_gc_header(plain_a_ptr as usize) }
                .expect("fresh string header");
        assert_eq!(
            plain_a_gc.gc_flags & crate::gc::GC_FLAG_INTERNED,
            0,
            "the coverage key must exercise a non-interned string-pool handle"
        );
        let plain_a =
            f64::from_bits(crate::value::STRING_TAG | (plain_a_ptr as u64 & POINTER_MASK));
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[plain_a], 2),
            1,
            "the once-only content lookup must not depend on unrelated interning"
        );
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
            (4u64 << 48) | (3u64 << 32) | (2u64 << 16) | 1,
            "four inline slots should be published in source order"
        );
        assert_eq!(
            object_array_numeric_write_guard(array_box, &[c, c, d], 2),
            (4u64 << 32) | (3u64 << 16) | 3,
            "duplicate target keys must preserve ordered duplicate stores"
        );
        assert_eq!(
            put_value::js_object_array_numeric_write2_guard(array_box, c, d, 2),
            (4u64 << 32) | 3,
            "the cached-object compatibility ABI must keep its 32-bit lanes"
        );
        assert_eq!(
            put_value::js_object_array_numeric_write_guard(array_box, a, b, c, d, 0, 2),
            0,
            "zero active fields must reject"
        );
        assert_eq!(
            put_value::js_object_array_numeric_write_guard(array_box, a, b, c, d, 5, 2),
            0,
            "field counts beyond the fixed descriptor must reject"
        );

        unsafe {
            let header =
                (second as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            let original = (*header)._reserved;

            for (flag, reason) in [
                (
                    crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
                    "descriptor-bearing receivers",
                ),
                (crate::gc::OBJ_FLAG_FROZEN, "frozen receivers"),
                (crate::gc::OBJ_FLAG_SEALED, "sealed receivers"),
                (crate::gc::OBJ_FLAG_NO_EXTEND, "non-extensible receivers"),
                (
                    crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO,
                    "typed-array-prototype receivers",
                ),
            ] {
                (*header)._reserved = original | flag;
                assert_eq!(
                    object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
                    0,
                    "{reason} must use ordinary [[Set]]"
                );
            }
            (*header)._reserved = original;
        }

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[a, b, c, d], 2),
            (4u64 << 48) | (3u64 << 32) | (2u64 << 16) | 1,
            "finite numbers are valid in verified ordinary JSValue typed slots"
        );

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[f64::NAN], 2),
            0,
            "non-interned/non-string keys must reject"
        );

        let hole_values = [boxed_object(first), f64::from_bits(crate::value::TAG_HOLE)];
        let hole_array =
            crate::array::js_array_from_f64(hole_values.as_ptr(), hole_values.len() as u32);
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(hole_array.cast()), &[c, d], 2),
            0,
            "a hole cannot be treated as an object receiver"
        );

        // Class ids alone do not distinguish layouts: equal ordered keys
        // canonicalize to the same array. Keep both targets present, but swap
        // their slots so publishing the first receiver's slots would be wrong.
        let other_packed = b"a\0b\0d\0c\0";
        let other_keys = crate::object::js_build_class_keys_array(
            0x6809_02,
            4,
            other_packed.as_ptr(),
            other_packed.len() as u32,
            0,
        );
        assert_ne!(
            keys, other_keys,
            "fixture requires different ordered key layouts"
        );
        let other = crate::object::js_object_alloc_class_inline_keys(0x6809_02, 0, 4, other_keys);
        let mixed_values = [boxed_object(first), boxed_object(other)];
        let mixed =
            crate::array::js_array_from_f64(mixed_values.as_ptr(), mixed_values.len() as u32);
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(mixed.cast()), &[c, d], 2),
            0,
            "different ordered key layouts must not share raw slots"
        );

        let ranged_values = [
            boxed_object(other),
            boxed_object(first),
            boxed_object(second),
        ];
        let ranged =
            crate::array::js_array_from_f64(ranged_values.as_ptr(), ranged_values.len() as u32);
        let ranged_box = boxed_object(ranged.cast());
        assert_eq!(
            object_array_numeric_write_range_guard(ranged_box, &[c, d], 1, 3),
            (4u64 << 16) | 3,
            "a non-zero range must ignore an ineligible receiver before its source start"
        );
        assert_eq!(
            object_array_numeric_write_guard(ranged_box, &[c, d], 3),
            0,
            "the legacy prefix ABI must continue proving from element zero"
        );
        assert_eq!(
            object_array_numeric_write_range_guard(ranged_box, &[c, d], 3, 3),
            0,
            "an empty receiver range must reject"
        );
        assert_eq!(
            object_array_numeric_write_range_guard(ranged_box, &[c, d], 1, 4),
            0,
            "a receiver range may not outrun the array"
        );

        unsafe {
            let original = (*first).class_id;
            let first_header =
                (first as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            let original_flags = (*first_header)._reserved;
            // Charter step 3: every per-object store fact below is changed
            // through its writer's transition, as the runtime changes it.
            (*first).class_id = 0;
            crate::object::shapes::store_kind::restamp_object_store_kind(first);
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[c, d], 2),
                0,
                "an UNMARKED class-id-zero object has no established ordinary-receiver \
                 identity and must use ordinary [[Set]]"
            );
            // #8098: the SAME receiver, birth-marked ORDINARY by the runtime —
            // which is what `JSON.parse` output carries — is eligible. The mark
            // is the only thing that differs between these two assertions, so
            // the pair discriminates "the guard reads the mark" from "the guard
            // stopped caring about class-id zero".
            (*first_header)._reserved = original_flags | crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
            crate::object::shapes::store_kind::restamp_object_store_kind(first);
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[c, d], 2),
                (4u64 << 16) | 3,
                "a marked ordinary plain object publishes the same raw slots as a \
                 class instance of the same shape"
            );
            // A native-module receiver stays out no matter what it is marked.
            (*first).class_id = crate::object::NATIVE_MODULE_CLASS_ID;
            crate::object::shapes::store_kind::restamp_object_store_kind(first);
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[c, d], 2),
                0,
                "a native-module receiver must reject even when marked ordinary"
            );
            (*first_header)._reserved = original_flags;
            (*first).class_id = original;
            crate::object::shapes::store_kind::restamp_object_store_kind(first);
        }

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[a, b, c, d], 3),
            0,
            "the preflight must reject a receiver prefix longer than the array"
        );

        let wide_packed = b"a\0b\0c\0d\0e\0";
        let wide_keys = crate::object::js_build_class_keys_array(
            0x6812_03,
            5,
            wide_packed.as_ptr(),
            wide_packed.len() as u32,
            0,
        );
        let narrow = crate::object::js_object_alloc_class_inline_keys(0x6812_03, 0, 4, wide_keys);
        let narrow_values = [boxed_object(narrow)];
        let narrow_array =
            crate::array::js_array_from_f64(narrow_values.as_ptr(), narrow_values.len() as u32);
        let e = boxed_interned_key(wide_keys, 4, b"e");
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(narrow_array.cast()), &[e], 1),
            0,
            "a key beyond the four-slot physical allocation must reject"
        );
    }

    /// #8098: the write PIC's generated hit path re-tests the ordinary-plain
    /// mark as a raw `_reserved` bit literal, so the runtime constant and the
    /// literal `perry-codegen/src/expr/proxy_reflect.rs` emits
    /// (`PLAIN_ORDINARY_OBJ_FLAG`) are one ABI. A silent divergence would make
    /// every generated guard test the wrong bit — either admitting a receiver
    /// the runtime never cleared, or never hitting at all. Pin the value here.
    #[test]
    fn plain_ordinary_object_flag_matches_the_emitted_write_pic_literal() {
        assert_eq!(
            crate::gc::OBJ_FLAG_PLAIN_ORDINARY,
            0x200,
            "perry-codegen emits 0x200 for this bit"
        );
        // It ADMITS a receiver, so it must not appear in the mask that REJECTS
        // one (`WRITE_PIC_BLOCKING_FLAGS = 0x1100`) — a collision would make
        // every marked object permanently ineligible.
        assert_eq!(crate::gc::OBJ_FLAG_PLAIN_ORDINARY & 0x1100, 0);
        // Bit 9 is shared with the array-only arguments-object flag, disjoint
        // by `obj_type`; and it must not collide with any object-meaningful
        // flag or with the survival-age field the GC owns.
        for other in [
            crate::gc::OBJ_FLAG_FROZEN,
            crate::gc::OBJ_FLAG_SEALED,
            crate::gc::OBJ_FLAG_NO_EXTEND,
            crate::gc::OBJ_FLAG_NULL_PROTO,
            crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO,
            crate::gc::OBJ_FLAG_HAS_DESCRIPTORS,
            0x0018, // GC_COPY_SURVIVAL_AGE_MASK
        ] {
            assert_eq!(
                crate::gc::OBJ_FLAG_PLAIN_ORDINARY & other,
                0,
                "the ordinary-plain mark must own its own bit"
            );
        }
    }

    /// #8098 end-to-end: real `JSON.parse` output must reach the whole-loop
    /// numeric write clone.
    ///
    /// This drives the shipped parser rather than hand-building a class-less
    /// object, because the property under test is that the PARSER marks what it
    /// allocates — a guard relaxation with no marking site would leave the
    /// matrix's `receiver_class_id_zero` cell exactly where it was.
    ///
    /// The 13-byte object payload is deliberately below the tape's 1 KB floor
    /// and has an object root, so `js_json_parse` takes the eager
    /// `DirectParser` (`json/parse_api.rs`) — no lazy tape stands between this
    /// probe and the objects it inspects (#7635).
    #[test]
    fn record_receivers_are_admitted_to_the_whole_loop_write_clone() {
        // This proof honours the process-wide class-field inline gate, and any
        // earlier test that ran `js_gc_init` (typed feedback is always on in
        // test builds) leaves that gate disabled. Establish the premise.

        let src = br#"{"x":0,"y":0}"#;
        let key_ptr = crate::string::js_string_from_bytes(b"y".as_ptr(), 1);
        let key_y = f64::from_bits(crate::value::STRING_TAG | (key_ptr as u64 & POINTER_MASK));
        // A JSON.parse record keeps its fields as fixed slots (no overflow),
        // so the layout proof serves it like any other record.
        let parsed: Vec<f64> = (0..4)
            .map(|_| put_value::record_fixture::json(src))
            .collect();
        let parsed_array = crate::array::js_array_from_f64(parsed.as_ptr(), parsed.len() as u32);
        assert_eq!(
            object_array_numeric_write_guard(boxed_object(parsed_array.cast()), &[key_y], 4),
            2,
            "JSON.parse records keep fixed slots and are admitted like any record"
        );
        // The admission itself, on the same records built with fixed slots.
        let mut receivers = Vec::new();
        for _ in 0..4 {
            receivers.push(put_value::record_fixture::record(src));
        }
        let array = crate::array::js_array_from_f64(receivers.as_ptr(), receivers.len() as u32);
        let array_box = boxed_object(array.cast());

        let objects: Vec<*mut crate::ObjectHeader> = receivers
            .iter()
            .map(|v| (v.to_bits() & POINTER_MASK) as *mut crate::ObjectHeader)
            .collect();
        unsafe {
            assert_eq!(
                (*objects[0]).class_id,
                0,
                "the premise: parsed receivers carry no class id"
            );
            assert_ne!(
                crate::object::shapes::object_shape_stamp(objects[0]),
                0,
                "the premise: #8067/#8086 birth-stamps them with a real ShapeId"
            );
            for object in &objects[1..] {
                assert_eq!(
                    crate::object::shapes::object_shape_stamp(*object),
                    crate::object::shapes::object_shape_stamp(objects[0]),
                    "records of one key sequence share one ShapeId"
                );
            }
        }

        assert_eq!(
            object_array_numeric_write_guard(array_box, &[key_y], 4),
            2,
            "the whole-loop clone must publish slot 1 for a record receiver prefix"
        );

        // The discriminating quantity: clear the ordinary mark on ONE receiver
        // and nothing else. Same objects, same keys, same slots — charter step
        // 3: the cleared mark moves that receiver to its `OrdinaryUnmarked`
        // twin, which is what the guard must refuse.
        unsafe {
            let header =
                (objects[2] as *mut u8).sub(crate::gc::GC_HEADER_SIZE) as *mut crate::gc::GcHeader;
            let saved = (*header)._reserved;
            assert_ne!(
                saved & crate::gc::OBJ_FLAG_PLAIN_ORDINARY,
                0,
                "the record is born marked ordinary"
            );
            (*header)._reserved = saved & !crate::gc::OBJ_FLAG_PLAIN_ORDINARY;
            crate::object::shapes::store_kind::restamp_object_store_kind(objects[2]);
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[key_y], 4),
                0,
                "one unmarked receiver in the prefix must send the whole nest to \
                 ordinary [[Set]]"
            );
            (*header)._reserved = saved;
            crate::object::shapes::store_kind::restamp_object_store_kind(objects[2]);
            assert_eq!(
                object_array_numeric_write_guard(array_box, &[key_y], 4),
                2,
                "restoring the mark restores eligibility"
            );
        }
    }

    /// #7531: `create_list_from_array_like` backs `Reflect.apply(target,
    /// thisArg, argumentsList)` / `Reflect.construct` -- `argumentsList` is
    /// caller-supplied and can be a fetch/zlib/proxy/common-registry handle
    /// id under the same POINTER_TAG as a real Array. The OLD "is this an
    /// Array?" fast path was a bare magnitude floor
    /// (`GC_HEADER_SIZE + 0x1000` = 0x1008) with NO other guard before
    /// dereferencing `addr - GC_HEADER_SIZE` -- every probe below is at/above
    /// that floor, so the old code would have derefed unconditionally.
    /// Calling the real function end-to-end (not just the predicate) proves
    /// the fast path is skipped without crashing; Node's semantics for a
    /// lengthless array-like is an empty list.
    #[test]
    fn create_list_from_array_like_rejects_handle_band_fast_path_without_dereferencing() {
        use crate::value::addr_class;
        const OLD_FLOOR: usize = 0x1008;
        let probes = [
            addr_class::COMMON_HANDLE_BAND_END,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::FETCH_HANDLE_BAND_END,
            addr_class::PROXY_ID_BAND_START,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        for addr in probes {
            assert!(
                addr >= OLD_FLOOR,
                "{addr:#x} must be at/above the old floor to prove the gap"
            );
            let boxed = f64::from_bits(crate::value::HANDLE_TAG | (addr as u64));
            assert!(
                create_list_from_array_like(boxed).is_empty(),
                "{addr:#x} must not be misread as a live Array"
            );
        }
    }

    #[test]
    fn create_list_from_array_like_unpacks_arguments_objects() {
        let raw = crate::array::js_array_alloc(3);
        let raw = crate::array::js_array_push_f64(raw, 11.0);
        let raw = crate::array::js_array_push_f64(raw, 22.0);
        let raw = crate::array::js_array_push_f64(raw, 33.0);
        let raw_args = crate::value::js_nanbox_pointer(raw as i64);
        let undefined = f64::from_bits(TAG_UNDEFINED);
        let arguments = crate::object::js_arguments_object_alloc(raw_args, undefined, 0);
        let boxed_arguments = crate::value::js_nanbox_pointer(arguments as i64);

        assert_eq!(
            create_list_from_array_like(boxed_arguments),
            vec![11.0, 22.0, 33.0],
            "Reflect.apply must preserve every entry from a real arguments object"
        );
    }

    /// #7531: `raw_ptr_from_value` feeds `array_ptr_from_value`, which derefs
    /// `raw - GC_HEADER_SIZE` right after a magnitude-only `is_valid_obj_ptr`
    /// guard. The OLD floor here (`GC_HEADER_SIZE + 0x1000`) sat below every
    /// handle band, and the local variable name `raw` made it invisible to
    /// `scripts/addr_class_inventory.py`'s `handle-floor` regex.
    #[test]
    fn array_ptr_from_value_rejects_every_handle_band_without_dereferencing() {
        use crate::value::addr_class;
        const OLD_FLOOR: usize = 0x1008;
        let probes = [
            addr_class::COMMON_HANDLE_BAND_END,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::FETCH_HANDLE_BAND_END,
            addr_class::PROXY_ID_BAND_START,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        for addr in probes {
            assert!(
                addr >= OLD_FLOOR,
                "{addr:#x} must be at/above the old floor to prove the gap"
            );
            let boxed = f64::from_bits(crate::value::HANDLE_TAG | (addr as u64));
            assert!(
                array_ptr_from_value(boxed).is_none(),
                "{addr:#x} must not resolve to an ArrayHeader"
            );
        }
    }

    /// #7531: `prototype_of_for_set` backs `Reflect.getPrototypeOf` /
    /// `is(value, type)` prototype walks. `lookup(value)` only recognizes an
    /// ALREADY-REGISTERED revocable Proxy id; any other handle-band value
    /// (fetch/zlib/common-registry, or a foreign Proxy id) falls through to
    /// the POINTER_TAG block, whose OLD floor
    /// (`GC_HEADER_SIZE + 0x1000`) admitted it into
    /// `object_static_prototype` (safe) and then a magnitude-only
    /// `is_valid_obj_ptr(obj)` guard followed by an unconditional
    /// `(*obj).class_id` read.
    #[test]
    fn prototype_of_for_set_rejects_every_handle_band_without_dereferencing() {
        use crate::value::addr_class;
        const OLD_FLOOR: usize = 0x1008;
        let probes = [
            addr_class::COMMON_HANDLE_BAND_END,
            addr_class::FETCH_HANDLE_BAND_START,
            addr_class::FETCH_HANDLE_BAND_END,
            addr_class::PROXY_ID_BAND_START,
            addr_class::HANDLE_BAND_MAX - 1,
        ];
        for addr in probes {
            assert!(
                addr >= OLD_FLOOR,
                "{addr:#x} must be at/above the old floor to prove the gap"
            );
            let boxed = f64::from_bits(crate::value::HANDLE_TAG | (addr as u64));
            // Reaching this assertion at all (no SIGSEGV) is half the test.
            let _ = prototype_of_for_set(boxed);
        }
    }
}

pub(crate) use reflect_misc::proxy_define_own_property_decoded;
