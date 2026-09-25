//! Compile-time-unresolved global references: read / optional read / update /
//! assign-existing against `globalThis`, throwing the spec ReferenceError when
//! the global does not exist. Split out of `error.rs` to keep it under the
//! 2,000-line cap (#10750); pure relocation.

use super::*;

/// Keepalive anchor for the auto-optimize whole-program build (generated-code
///-only callee; see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_GLOBAL_GET_OR_THROW_UNRESOLVED: extern "C-unwind" fn(f64) -> f64 =
    js_global_get_or_throw_unresolved;

/// Read a compile-time-unresolved identifier off `globalThis` (a global the
/// program created dynamically — `Function("this.y = 2")()` — exists only at
/// runtime), throwing the spec ReferenceError when no such global property
/// exists.
///
/// `C-unwind` is required because the missing-global path raises a Perry
/// exception through the system unwinder to generated code's landing pad. A
/// plain `extern "C"` boundary aborts before a surrounding JavaScript `catch`
/// can run when the debug runtime is linked.
#[no_mangle]
pub extern "C-unwind" fn js_global_get_or_throw_unresolved(name_value: f64) -> f64 {
    // `js_get_global_this` lazily builds the realm on first use and can collect
    // while doing so. Generated code may pass a nursery string here from a
    // registered module-root slot, but this argument is only a copied NaN-box:
    // the collector rewrites the slot, not this Rust local. Root it before the
    // first allocation and reload it at every later GC-capable boundary.
    let scope = crate::gc::RuntimeHandleScope::new();
    let name_handle = scope.root_nanbox_f64(name_value);
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    if gj.is_pointer() {
        // #6943: `js_string_coerce` allocates for every non-heap-string name,
        // so it can trigger a GC that **evacuates**. The global object's header
        // was extracted into a raw Rust local *before* the coercion and
        // dereferenced by `js_object_get_field_by_name` after it. Root the
        // receiver and re-derive the header from the refreshed value.
        let g_handle = scope.root_heap_word_u64(g.to_bits());
        let key = crate::builtins::js_string_coerce(name_handle.get_nanbox_f64());
        let g = f64::from_bits(g_handle.get_heap_word_u64());
        let gptr = (g.to_bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        if !gptr.is_null() && !key.is_null() {
            let v = crate::object::js_object_get_field_by_name(gptr, key);
            if !v.is_undefined() {
                return f64::from_bits(v.bits());
            }
            // A global binding initialized to `undefined` (a sloppy global var
            // created by `f = undefined`, or B.3.3.3 CreateGlobalVarBinding from
            // eval'd function hoisting) is *resolvable* — reading it yields
            // `undefined`, not a ReferenceError. `js_object_get_field_by_name`
            // can't tell "absent" from "present, value undefined", so confirm
            // the property actually exists (as an OWN property — a global var
            // binding always is) before falling through to the throw.
            let has = crate::object::js_object_has_own(
                f64::from_bits(g_handle.get_heap_word_u64()),
                name_handle.get_nanbox_f64(),
            );
            if crate::value::js_is_truthy(has) != 0 {
                return f64::from_bits(crate::value::JSValue::undefined().bits());
            }
        }
    }
    let name = value_to_lossy_string(name_handle.get_nanbox_f64());
    let msg = format!("{} is not defined", name);
    let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err_ptr = js_referenceerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64))
}

const _: extern "C-unwind" fn(f64) -> f64 = js_global_get_or_throw_unresolved;

/// Keepalive anchor for the auto-optimize whole-program build (generated-code
///-only callee; see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_GLOBAL_GET_OPTIONAL: extern "C" fn(f64) -> f64 = js_global_get_optional;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_GLOBAL_UPDATE: extern "C" fn(f64, f64, f64) -> f64 = js_global_update;

/// `++x` / `x++` / `--x` / `x--` where `x` resolves to no lexical binding —
/// i.e. a (sloppy) global property reference. Read globalThis[name] (throwing
/// the spec ReferenceError when the property is absent — a genuinely
/// unresolvable reference, e.g. `++neverDeclared`), ToNumeric, step by 1 of
/// its own type, write the result back to globalThis, and return the
/// post-step value for a prefix op or the pre-step ToNumeric value for a
/// postfix op (#3575: `for (i = 0; i < n; i++)` with an undeclared `i`).
/// The boolean flags arrive NaN-boxed (codegen passes HIR `Bool` literals).
#[no_mangle]
pub extern "C" fn js_global_update(name_value: f64, is_increment: f64, is_prefix: f64) -> f64 {
    let is_increment = crate::value::js_is_truthy(is_increment);
    let is_prefix = crate::value::js_is_truthy(is_prefix) != 0;
    let scope = crate::gc::RuntimeHandleScope::new();
    let name_handle = scope.root_nanbox_f64(name_value);
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    // #6943: `js_string_coerce` allocates for every non-heap-string name, and
    // the read-modify-write below adds `js_object_get_field_by_name`,
    // `js_object_has_own`, `js_to_numeric` and `js_numeric_step` — every one of
    // them GC-capable. The global object (`g`, and the `gptr` header derived
    // from the pre-coercion `gj`) and the coerced key string were raw Rust
    // locals across all of it, and `gptr` is the receiver of the WRITE-BACK at
    // the end. Root both and re-derive the header at each use.
    let g_handle = scope.root_heap_word_u64(g.to_bits());
    let key = crate::builtins::js_string_coerce(name_handle.get_nanbox_f64());
    let key_handle = scope.root_string_ptr(key);
    let mut present = false;
    let old = if gj.is_pointer() && !key.is_null() {
        let g = f64::from_bits(g_handle.get_heap_word_u64());
        let gptr = (g.to_bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        if !gptr.is_null() {
            let v = crate::object::js_object_get_field_by_name(
                gptr,
                key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
            );
            if !v.is_undefined()
                || crate::object::js_object_has_own(
                    f64::from_bits(g_handle.get_heap_word_u64()),
                    name_handle.get_nanbox_f64(),
                )
                .to_bits()
                    == crate::value::TAG_TRUE
            {
                present = true;
            }
            f64::from_bits(v.bits())
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    } else {
        f64::from_bits(crate::value::TAG_UNDEFINED)
    };
    if !present {
        let name = value_to_lossy_string(name_handle.get_nanbox_f64());
        let msg = format!("{} is not defined", name);
        let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = js_referenceerror_new(msg_str);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
    }
    let old_handle = scope.root_nanbox_f64(old);
    let numeric = unsafe { crate::value::js_to_numeric(old_handle.get_nanbox_f64()) };
    let numeric_handle = scope.root_nanbox_f64(numeric);
    let stepped =
        unsafe { crate::value::js_numeric_step(numeric_handle.get_nanbox_f64(), is_increment) };
    let stepped_handle = scope.root_nanbox_f64(stepped);
    let g = f64::from_bits(g_handle.get_heap_word_u64());
    let gptr = (g.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
    crate::object::js_object_set_field_by_name(
        gptr,
        key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
        stepped_handle.get_nanbox_f64(),
    );
    let numeric = numeric_handle.get_nanbox_f64();
    let stepped = stepped_handle.get_nanbox_f64();
    if is_prefix {
        stepped
    } else {
        numeric
    }
}

/// Keepalive anchor for the auto-optimize whole-program build (generated-code
///-only callee; see project_auto_optimize_keepalive_3320).
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_GLOBAL_ASSIGN_EXISTING_OR_THROW: extern "C" fn(f64, f64) -> f64 =
    js_global_assign_existing_or_throw;

/// Strict-mode assignment to an identifier with no lexical binding
/// (#5989). Per spec (PutValue on an unresolvable-in-strict reference),
/// the name must first resolve against the global object: an EXISTING
/// global property is a normal property write — Next.js 16's
/// `cacheComponents` node-environment extensions do exactly this
/// (`Date = createDate(Date)` in strict CJS to install the dynamic-IO
/// clock interceptor, likewise `crypto`/`Math.random` wrappers) — and
/// only a genuinely absent binding throws the ReferenceError. The old
/// lowering threw unconditionally, so the extension install failed at
/// boot ("Failed to install `Date` class extension") and the dynamic
/// prerender-abort chain never armed. Presence probing + write-back
/// mirror `js_global_update` (the `++x`-on-global sibling). Sloppy mode
/// never reaches this helper — it lowers to a globalThis property set
/// that may CREATE the binding.
#[no_mangle]
pub extern "C" fn js_global_assign_existing_or_throw(name_value: f64, value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name_handle = scope.root_nanbox_f64(name_value);
    let value_handle = scope.root_nanbox_f64(value);
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    // #6943: the textbook shape of this family — a receiver AND the value being
    // stored into it, both raw across a GC-capable `js_string_coerce`. The
    // presence probe (`js_object_get_field_by_name`, `js_object_has_own`) and
    // the not-defined path (`js_string_from_bytes`, `js_referenceerror_new`)
    // allocate on top of that, and `gptr` is the receiver of the final write.
    // Root the global, the coerced key and `value` for the whole helper.
    let g_handle = scope.root_heap_word_u64(g.to_bits());
    let key = crate::builtins::js_string_coerce(name_handle.get_nanbox_f64());
    let key_handle = scope.root_string_ptr(key);
    let mut present = false;
    if gj.is_pointer() && !key.is_null() {
        let g = f64::from_bits(g_handle.get_heap_word_u64());
        let gptr = (g.to_bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        if !gptr.is_null() {
            let v = crate::object::js_object_get_field_by_name(
                gptr,
                key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
            );
            if !v.is_undefined()
                || crate::object::js_object_has_own(
                    f64::from_bits(g_handle.get_heap_word_u64()),
                    name_handle.get_nanbox_f64(),
                )
                .to_bits()
                    == crate::value::TAG_TRUE
            {
                present = true;
            }
        }
    }
    if !present {
        let name = value_to_lossy_string(name_handle.get_nanbox_f64());
        let msg = format!("{} is not defined", name);
        let msg_str = js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
        let err_ptr = js_referenceerror_new(msg_str);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err_ptr as i64));
    }
    let g = f64::from_bits(g_handle.get_heap_word_u64());
    let gptr = (g.to_bits() & crate::value::POINTER_MASK) as *mut crate::object::ObjectHeader;
    crate::object::js_object_set_field_by_name(
        gptr,
        key_handle.get_raw_const_ptr::<crate::string::StringHeader>(),
        value_handle.get_nanbox_f64(),
    );
    // An assignment expression evaluates to its RHS.
    value_handle.get_nanbox_f64()
}

/// Non-throwing variant of [`js_global_get_or_throw_unresolved`] for
/// `typeof <unresolved ident>`: the spec's GetValue-skips-on-typeof rule means
/// a missing global yields `undefined` rather than a ReferenceError, but a
/// global created at RUNTIME (sloppy `foo = 1` lowers to a globalThis
/// property set — #3575) must still be observed.
#[no_mangle]
pub extern "C" fn js_global_get_optional(name_value: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name_handle = scope.root_nanbox_f64(name_value);
    let g = crate::object::js_get_global_this();
    let gj = crate::value::JSValue::from_bits(g.to_bits());
    if gj.is_pointer() {
        // #6943: root the global across the GC-capable coercion and re-derive
        // its header afterwards — see `js_global_get_or_throw_unresolved`.
        let g_handle = scope.root_heap_word_u64(g.to_bits());
        let key = crate::builtins::js_string_coerce(name_handle.get_nanbox_f64());
        let g = f64::from_bits(g_handle.get_heap_word_u64());
        let gptr = (g.to_bits() & crate::value::POINTER_MASK) as *const crate::object::ObjectHeader;
        if !gptr.is_null() && !key.is_null() {
            let v = crate::object::js_object_get_field_by_name(gptr, key);
            return f64::from_bits(v.bits());
        }
    }
    f64::from_bits(crate::value::TAG_UNDEFINED)
}
