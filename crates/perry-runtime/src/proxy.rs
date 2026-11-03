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

pub(crate) mod apply_construct;
pub use apply_construct::{call_proxy_value_with_this, js_proxy_apply, js_proxy_construct};
pub(crate) use apply_construct::{is_callable_function, is_constructor_function};
pub(crate) mod get;
pub use get::js_proxy_get;
pub(crate) mod has_delete;
pub use has_delete::{js_proxy_delete, js_proxy_has};
mod invariants;
#[cfg(test)]
mod receiver_definition_tests;
mod set_interceptor;
pub(crate) use set_interceptor::set_interceptor;
pub(crate) mod put_value;
pub(crate) use put_value::IC_SLOT_OVERFLOW_BIT;
pub use put_value::{js_proxy_set, js_put_value_set, js_put_value_set_dynamic};
pub(crate) mod json;
pub(crate) mod metadata;
pub(crate) mod own_keys;
pub(crate) mod prototype;
pub(crate) mod reflect;
pub(crate) mod reflect_misc;
pub use reflect_misc::{
    js_reflect_apply, js_reflect_construct, js_reflect_define_property,
    js_reflect_get_prototype_of, js_reflect_is_extensible, js_reflect_own_keys,
    js_reflect_prevent_extensions,
};

pub use own_keys::js_proxy_own_keys;
pub use prototype::js_reflect_set_prototype_of;

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
#[cfg(test)]
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// A Proxy is a nonmoving GC cell of kind `GC_TYPE_PROXY`, boxed under
/// `POINTER_TAG` like any object (honest pointers), so its header byte is the
/// whole proof. Returns the cell address.

#[inline]
fn lookup(proxy_boxed: f64) -> Option<u64> {
    let bits = proxy_boxed.to_bits();
    let header = crate::value::addr_class::pointer_value_header(bits)?;
    is_proxy_header(header).then_some(bits & POINTER_MASK)
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
    use crate::codegen_abi::LayoutWord;

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
        bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
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
                LayoutWord(0),
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

pub(crate) use crate::object::property_ops::common::{
    closure_from, create_list_from_array_like, is_callable, legacy_dunder_proto_set, nanbox_bool,
    reflect_value_is_object, throw_type_error, value_display_string,
};

pub use crate::object::property_ops::js_super_put_value_set;
pub(crate) use crate::object::property_ops::*;

pub(crate) fn proxy_is_constructor(value: f64) -> bool {
    lookup(value)
        .and_then(entry)
        .is_some_and(|e| e.constructable)
}

pub(crate) use crate::proxy_hooks::is_proxy_header;
