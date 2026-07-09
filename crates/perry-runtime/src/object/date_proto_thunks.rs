//! `Date.prototype` getter thunks with a `this` brand check.
//!
//! The instance fast path (`d.getFullYear()`) is lowered directly by codegen:
//! it reads the `DateCell` timestamp and calls the `js_date_get_*` helpers, so
//! it never touches these thunks. But `Date.prototype.getFullYear` is also a
//! plain value — `Date.prototype.getFullYear.call(x)`, `Reflect.apply`, method
//! extraction — and on that reflective path the methods were installed as
//! generic no-op thunks, so `getDate.call(realDate)` returned `[object Object]`
//! and `getDate.call(nonDate)` silently produced garbage instead of throwing.
//!
//! Per spec each `Date.prototype` getter performs `thisTimeValue(this)`, which
//! throws a `TypeError` when `this` is not an Object with a `[[DateValue]]`
//! slot. These thunks read the `IMPLICIT_THIS` receiver (set by the
//! `.call`/`.apply` dispatch), brand-check it via `is_date_value`, throw on
//! mismatch, and otherwise dispatch to the SAME `js_date_get_*` helper the
//! instance path uses — so reflective Date getter calls now also *work*.
//!
//! Installed onto `Date.prototype` by
//! `global_this::populate_builtin_prototype_methods` (after the no-op block, so
//! these real thunks overwrite the no-op getters; setters / `toX` formatters
//! stay on the no-op path for now).

use super::*;

/// Resolve the `IMPLICIT_THIS` receiver to a Date time value, or throw a
/// `TypeError` (`thisTimeValue` brand check) when it is not a Date.
fn require_date_timestamp() -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if crate::date::is_date_value(this) {
        crate::date::date_cell_timestamp(this)
    } else {
        super::object_ops::throw_object_type_error(b"this is not a Date object.")
    }
}

macro_rules! date_getter_thunk {
    ($name:ident, $rt:path) => {
        extern "C" fn $name(_closure: *const crate::closure::ClosureHeader) -> f64 {
            $rt(require_date_timestamp())
        }
    };
}

date_getter_thunk!(date_get_time, crate::date::js_date_get_time);
date_getter_thunk!(date_get_full_year, crate::date::js_date_get_full_year);
date_getter_thunk!(date_get_month, crate::date::js_date_get_month);
date_getter_thunk!(date_get_date, crate::date::js_date_get_date);
date_getter_thunk!(date_get_hours, crate::date::js_date_get_hours);
date_getter_thunk!(date_get_minutes, crate::date::js_date_get_minutes);
date_getter_thunk!(date_get_seconds, crate::date::js_date_get_seconds);
date_getter_thunk!(date_get_milliseconds, crate::date::js_date_get_milliseconds);
date_getter_thunk!(date_get_day, crate::date::js_date_get_day);
date_getter_thunk!(
    date_get_utc_full_year,
    crate::date::js_date_get_utc_full_year
);
date_getter_thunk!(date_get_utc_month, crate::date::js_date_get_utc_month);
date_getter_thunk!(date_get_utc_date, crate::date::js_date_get_utc_date);
date_getter_thunk!(date_get_utc_hours, crate::date::js_date_get_utc_hours);
date_getter_thunk!(date_get_utc_minutes, crate::date::js_date_get_utc_minutes);
date_getter_thunk!(date_get_utc_seconds, crate::date::js_date_get_utc_seconds);
date_getter_thunk!(
    date_get_utc_milliseconds,
    crate::date::js_date_get_utc_milliseconds
);
date_getter_thunk!(date_get_utc_day, crate::date::js_date_get_utc_day);
date_getter_thunk!(
    date_get_timezone_offset,
    crate::date::js_date_get_timezone_offset
);

/// `Date.prototype.toISOString` reflective thunk. The instance fast path
/// (`d.toISOString()`) is lowered by codegen straight to
/// `js_date_to_iso_string_or_throw`, but the value form
/// (`Date.prototype.toISOString.call(x)`) reaches here: brand-check `this`
/// (TypeError on a non-Date receiver), then format-or-throw (RangeError when
/// the time value is `NaN`).
extern "C" fn date_to_iso_string(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if !crate::date::is_date_value(this) {
        super::object_ops::throw_object_type_error(b"this is not a Date object.");
    }
    let s = crate::date::js_date_to_iso_string_or_throw(this);
    crate::value::js_nanbox_string(s as i64)
}

/// `Date.prototype.toUTCString` (and its legacy `toGMTString` alias)
/// reflective thunk: brand-check `this` (TypeError on a non-Date receiver),
/// then format as "Sun, 23 Mar 2014 00:00:00 GMT" (or "Invalid Date").
extern "C" fn date_to_utc_string(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if !crate::date::is_date_value(this) {
        super::object_ops::throw_object_type_error(b"this is not a Date object.");
    }
    let s = crate::date::js_date_to_utc_string(this);
    crate::value::js_nanbox_string(s as i64)
}

/// `Date.prototype.toJSON` reflective thunk. Unlike the getters this is a
/// *generic* method — it is not brand-checked to Date. Per ECMA-262
/// (`thisTimeValue` is NOT used): `ToObject(this)`, then `ToPrimitive(this,
/// number)`; if that primitive is a Number and not finite, return `null`;
/// otherwise return `Invoke(this, "toISOString")`. So it works for a real Date
/// (Invalid → `null`), a plain object carrying its own `toISOString`, and a
/// `Number(-Infinity)` wrapper (→ `null`).
extern "C" fn date_to_json(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    let jsv = crate::value::JSValue::from_bits(this.to_bits());
    // ToObject(this): null / undefined throw a TypeError.
    if jsv.is_undefined() || jsv.is_null() {
        super::object_ops::throw_object_type_error(b"Cannot convert undefined or null to object");
    }
    // ToPrimitive(this, hint Number).
    let tv = if crate::date::is_date_value(this) {
        crate::date::date_cell_timestamp(this)
    } else {
        match unsafe { crate::value::to_primitive_number(this) } {
            crate::value::OrdinaryToPrimitiveOutcome::Primitive(p) => p,
            // A plain object's default `"[object Object]"` is a String, never a
            // Number, so step 3 (non-finite Number → null) never fires; fall
            // through to the `toISOString` invocation.
            crate::value::OrdinaryToPrimitiveOutcome::DefaultString => {
                f64::from_bits(crate::value::TAG_UNDEFINED)
            }
            crate::value::OrdinaryToPrimitiveOutcome::TypeError => {
                super::object_ops::throw_object_type_error(
                    b"Cannot convert object to primitive value",
                )
            }
        }
    };
    // Step 3: if the primitive is a Number and is not finite, return null.
    let tv_jsv = crate::value::JSValue::from_bits(tv.to_bits());
    let as_num = if tv_jsv.is_int32() {
        Some(((tv.to_bits() & 0xFFFF_FFFF) as u32 as i32) as f64)
    } else if tv_jsv.is_number() {
        Some(tv)
    } else {
        None
    };
    if let Some(n) = as_num {
        if !n.is_finite() {
            return f64::from_bits(crate::value::TAG_NULL);
        }
    }
    // Step 4: Invoke(this, "toISOString"). For a real Date, dispatch straight
    // to the runtime helper: `js_native_call_method` does not resolve the
    // reflective `toISOString` on a `DateCell` receiver and would fall back to
    // the generic `[object Object]` Object.prototype.toString. Other receivers
    // (a plain object carrying its own `toISOString`) use the ordinary Invoke.
    if crate::date::is_date_value(this) {
        let s = crate::date::js_date_to_iso_string_or_throw(this);
        return crate::value::js_nanbox_string(s as i64);
    }
    // Other receivers: a full `Invoke(O, "toISOString")` where
    // `O = ToObject(this)`. `ToObject` boxes primitives (so
    // `Date.prototype.toJSON.call(10)` reaches `Number.prototype.toISOString`),
    // the property read fires accessor getters and walks the prototype chain
    // (`{ get toISOString() {…} }`), and a non-callable result throws
    // `TypeError` from `Call`. `js_native_call_method` does none of these — it
    // only dispatches builtin/native methods — so it is not used here.
    let o = crate::object::js_object_coerce(this);
    let key = {
        let k = crate::string::js_string_from_bytes(b"toISOString".as_ptr(), 11);
        f64::from_bits(crate::value::js_nanbox_string(k as i64).to_bits())
    };
    let func = crate::proxy::js_reflect_get(o, key, o);
    let closure = crate::value::js_nanbox_get_pointer(func) as *const crate::closure::ClosureHeader;
    if crate::collection_iter::is_callable(func)
        && !crate::closure::get_valid_func_ptr(closure).is_null()
    {
        // `Call(func, O, «»)` — toJSON's `key` argument is intentionally not
        // forwarded (Invoke passes an empty argument list).
        let prev = crate::object::js_implicit_this_set(o);
        let r = crate::closure::js_closure_call0(closure);
        crate::object::js_implicit_this_set(prev);
        return r;
    }
    super::object_ops::throw_object_type_error(b"toISOString is not a function")
}

#[cfg(test)]
pub(crate) fn test_date_to_json_current_this() -> f64 {
    date_to_json(std::ptr::null())
}

/// True iff `value` is an ECMAScript Object (`Type(O) is Object`). Objects are
/// the pointer-tagged heap values EXCEPT registered `Symbol`s (which share the
/// pointer band) and `BigInt`s (distinct tag). All the primitive tags
/// (undefined/null/bool/number/int32/string/bigint/symbol) are NOT Objects.
fn value_is_object(value: f64) -> bool {
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if !jsv.is_pointer() {
        return false;
    }
    let raw = (value.to_bits() & crate::value::POINTER_MASK) as usize;
    !crate::symbol::is_registered_symbol(raw)
}

/// `Date.prototype [ @@toPrimitive ] ( hint )` (ECMA-262 §21.4.4.45).
///
/// Unlike the other `Date.prototype` methods this is NOT brand-checked to a
/// Date and does NOT read the time value — it is a *generic* `OrdinaryToPrimitive`
/// dispatcher installed on `Date.prototype` and invoked with `this` = whatever
/// the caller supplies (`.call(obj, hint)`). Steps:
///   1. Let O be the `this` value. If `Type(O)` is not Object, throw a TypeError.
///   2. If `hint` is "string" or "default" → `tryFirst = string`.
///   3. Else if `hint` is "number" → `tryFirst = number`.
///   4. Else throw a TypeError.
///   5. Return `OrdinaryToPrimitive(O, tryFirst)`.
///
/// The `hint` comparison is against the primitive String value only: a
/// `new String("number")` wrapper or any non-string is an invalid hint
/// (TypeError), matching test262 `hint-invalid.js`.
extern "C" fn date_to_primitive(_closure: *const crate::closure::ClosureHeader, hint: f64) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    // Step 1: brand-check `Type(O) is Object` (throws for undefined/null/86/''/true).
    if !value_is_object(this) {
        super::object_ops::throw_object_type_error(
            b"Date.prototype[Symbol.toPrimitive] called on non-object",
        );
    }
    // Steps 2-4: resolve `tryFirst` from the primitive-string hint.
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let try_string_first = match crate::string::str_bytes_from_jsvalue(hint, &mut scratch) {
        Some((ptr, len)) => {
            let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
            match bytes {
                b"string" | b"default" => true,
                b"number" => false,
                _ => super::object_ops::throw_object_type_error(
                    b"Date.prototype[Symbol.toPrimitive] called with invalid hint",
                ),
            }
        }
        // Not a primitive string (undefined, a `new String(...)` wrapper, …).
        None => super::object_ops::throw_object_type_error(
            b"Date.prototype[Symbol.toPrimitive] called with invalid hint",
        ),
    };
    // Step 5: OrdinaryToPrimitive(O, tryFirst).
    unsafe { crate::value::ordinary_to_primitive_for_toprimitive(this, try_string_first) }
}

/// Install `Date.prototype[Symbol.toPrimitive]` — a non-enumerable own method
/// keyed by the real well-known `Symbol.toPrimitive` (not an `@@`-string own
/// property, which would leak into `getOwnPropertyNames`). Its property
/// descriptor is `{ writable: false, enumerable: false, configurable: true }`
/// and the function's own `name` is `"[Symbol.toPrimitive]"` with `.length` 1
/// (test262 `built-ins/Date/prototype/Symbol.toPrimitive/{prop-desc,name,
/// length}.js`). Called from `populate_builtin_prototype_methods`'s `"Date"`
/// arm.
pub(crate) fn install_date_proto_to_primitive(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    let func_ptr = date_to_primitive as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 1);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    super::native_module::set_bound_native_closure_name(closure, "[Symbol.toPrimitive]");
    super::native_module::set_builtin_closure_length(closure as usize, 1);
    super::native_module::set_builtin_closure_non_constructable(closure as usize);
    // The function's own `name`/`length` are `{ writable:false, enumerable:false,
    // configurable:true }` per ES §17.
    let name_len_attrs = super::PropertyAttrs::new(false, false, true);
    super::set_builtin_property_attrs(closure as usize, "name".to_string(), name_len_attrs);
    super::set_builtin_property_attrs(closure as usize, "length".to_string(), name_len_attrs);
    let sym = crate::symbol::well_known_symbol("toPrimitive");
    if sym.is_null() {
        return;
    }
    let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
    let sym_value = f64::from_bits(crate::value::JSValue::pointer(sym as *const u8).bits());
    let fn_value = crate::value::js_nanbox_pointer(closure as i64);
    unsafe {
        crate::symbol::js_object_set_symbol_property(proto_value, sym_value, fn_value);
    }
    // The property is `{ writable:false, enumerable:false, configurable:true }`
    // (test262 `prop-desc.js`). Symbol props default to
    // `{ writable:true, enumerable:true, configurable:true }`, so record the
    // spec attrs explicitly.
    let sym_key = unsafe { crate::symbol::sym_key_from_f64(sym_value) };
    let proto_key = proto_obj as usize;
    crate::symbol::set_symbol_property_attrs(
        proto_key,
        sym_key,
        super::PropertyAttrs::new(false, false, true),
    );
}

// --- Date constructor static methods (Date.now / Date.parse / Date.UTC) ---
//
// The functional call forms are codegen intrinsics (`Expr::DateNow` /
// `DateParse` / `DateUtc`), recognized at HIR lowering before any property
// lookup, so these closures never intercept `Date.now()` etc. They exist only
// so the statics are real own properties of the `Date` constructor — readable
// as values (`const f = Date.UTC; f(...)`) and observable through reflection
// (`Date.UTC.name === "UTC"`, `Date.UTC.length === 7`,
// `getOwnPropertyDescriptor(Date, "UTC")`).

/// `Date.now()` — current time in milliseconds.
extern "C" fn date_now_static(_closure: *const crate::closure::ClosureHeader) -> f64 {
    crate::date::js_date_now()
}

/// `Date.parse(string)` — ToString the argument, then parse to a ms timestamp.
extern "C" fn date_parse_static(_closure: *const crate::closure::ClosureHeader, arg: f64) -> f64 {
    let s = crate::value::js_jsvalue_to_string(arg);
    crate::date::js_date_parse(s as *const crate::string::StringHeader)
}

/// `Date.UTC(year, month?, day?, …)` — variadic; forwards to `js_date_utc`.
extern "C" fn date_utc_static(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
    let vals = super::global_this::global_this_rest_array_values(rest);
    crate::date::js_date_utc(vals.as_ptr(), vals.len() as i32)
}

/// Install `now` / `parse` / `UTC` as own data properties on the `Date`
/// constructor closure. Called from `install_builtin_constructor_statics`.
pub(crate) fn install_date_constructor_statics(ctor: *mut crate::closure::ClosureHeader) {
    if ctor.is_null() {
        return;
    }
    // (name, fn-ptr, spec `.length`, has_rest)
    super::global_this::install_constructor_static(
        ctor,
        "now",
        date_now_static as *const u8,
        0,
        false,
    );
    super::global_this::install_constructor_static(
        ctor,
        "parse",
        date_parse_static as *const u8,
        1,
        false,
    );
    // `date_utc_static` collects *all* arguments into its `rest` param, so the
    // call-arity (fixed params before the rest) must be 0 — otherwise the rest
    // registration reserves 7 fixed slots and `Date.UTC(2020, 0)` (fewer than 7
    // args) puts nothing in the rest array → js_date_utc([]) → NaN. The spec
    // `.length` stays 7. (#4596)
    super::global_this::install_constructor_static_with_call_arity(
        ctor,
        "UTC",
        date_utc_static as *const u8,
        7,
        0,
        true,
    );
}

/// Legacy `Date.prototype.getYear` — `getFullYear() - 1900` (NaN-preserving).
extern "C" fn date_get_year(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let fy = crate::date::js_date_get_full_year(require_date_timestamp());
    if fy.is_nan() {
        fy
    } else {
        fy - 1900.0
    }
}

/// Install the brand-checked `Date.prototype` getter thunks. Called from
/// `global_this::populate_builtin_prototype_methods`'s `"Date"` arm AFTER the
/// no-op block, so these overwrite the no-op getter entries.
pub(crate) fn install_date_proto_getters(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    let methods: &[(&str, *const u8)] = &[
        ("getTime", date_get_time as *const u8),
        ("valueOf", date_get_time as *const u8),
        ("getFullYear", date_get_full_year as *const u8),
        ("getMonth", date_get_month as *const u8),
        ("getDate", date_get_date as *const u8),
        ("getHours", date_get_hours as *const u8),
        ("getMinutes", date_get_minutes as *const u8),
        ("getSeconds", date_get_seconds as *const u8),
        ("getMilliseconds", date_get_milliseconds as *const u8),
        ("getDay", date_get_day as *const u8),
        ("getUTCFullYear", date_get_utc_full_year as *const u8),
        ("getUTCMonth", date_get_utc_month as *const u8),
        ("getUTCDate", date_get_utc_date as *const u8),
        ("getUTCHours", date_get_utc_hours as *const u8),
        ("getUTCMinutes", date_get_utc_minutes as *const u8),
        ("getUTCSeconds", date_get_utc_seconds as *const u8),
        ("getUTCMilliseconds", date_get_utc_milliseconds as *const u8),
        ("getUTCDay", date_get_utc_day as *const u8),
        ("getTimezoneOffset", date_get_timezone_offset as *const u8),
        ("getYear", date_get_year as *const u8),
    ];
    for (name, ptr) in methods.iter().copied() {
        super::global_this::install_proto_method(proto_obj, name, ptr, 0);
    }
    // String-returning / generic methods that also need real reflective thunks
    // (overwriting their no-op entries). `toJSON` is generic (`.length === 1`),
    // `toISOString` brand-checks `this` (`.length === 0`).
    super::global_this::install_proto_method(
        proto_obj,
        "toISOString",
        date_to_iso_string as *const u8,
        0,
    );
    super::global_this::install_proto_method(proto_obj, "toJSON", date_to_json as *const u8, 1);
    let utc = super::global_this::install_proto_method(
        proto_obj,
        "toUTCString",
        date_to_utc_string as *const u8,
        0,
    );
    // Annex B: `Date.prototype.toGMTString` is the SAME function object as
    // `toUTCString` (`toGMTString === toUTCString`, `.name === "toUTCString"`),
    // not an independent thunk (test262 `annexB/built-ins/Date/.../toGMTString/
    // value`, #5346).
    super::global_this::install_proto_method_alias(proto_obj, "toGMTString", utc);
}

// --- Setters ---------------------------------------------------------------
//
// Like the getters, the instance fast path (`d.setHours(...)`) is lowered by
// codegen straight to `js_date_apply_setter`. But `Date.prototype.setHours` is
// also a plain value (`.call`/`.apply`, method extraction, the legacy
// `setYear`/`setUTC*` family with no codegen fast path), and on that reflective
// path the methods were generic no-ops: `setDate.call(realDate, 1)` no-op'd and
// `setDate.call(nonDate, 1)` silently produced garbage instead of throwing.
//
// Each setter performs `thisTimeValue(this)` (TypeError if `this` is not a
// Date) and is variadic, so these thunks read the `IMPLICIT_THIS` receiver,
// brand-check it, collect the `rest` arguments, and dispatch to the same
// `js_date_apply_setter` the instance path uses. `js_date_apply_setter` reads
// `[[DateValue]]` BEFORE coercing the arguments, so the read-before-ToNumber
// ordering holds on the reflective path too.

/// Resolve the `IMPLICIT_THIS` receiver to a Date value, or throw a `TypeError`
/// (`thisTimeValue` brand check) when it is not a Date. Returns the NaN-boxed
/// `DateCell` value (the setter dispatch needs the receiver itself, not just
/// its timestamp, so it can mutate the cell in place).
fn require_date_this() -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if crate::date::is_date_value(this) {
        this
    } else {
        super::object_ops::throw_object_type_error(b"this is not a Date object.")
    }
}

/// `field`/`is_utc` selectors mirror `crate::date::js_date_apply_setter`:
/// 0=FullYear 1=Month 2=Date 3=Hours 4=Minutes 5=Seconds 6=Milliseconds
/// 7=Time, plus 8=setYear (annexB; local-only). `is_utc != 0` picks the UTC
/// rebuild.
macro_rules! date_setter_thunk {
    ($name:ident, $is_utc:expr, $field:expr) => {
        extern "C" fn $name(_closure: *const crate::closure::ClosureHeader, rest: f64) -> f64 {
            let this = require_date_this();
            let args = super::global_this::global_this_rest_array_values(rest);
            crate::date::js_date_apply_setter(
                this,
                $is_utc,
                $field,
                args.as_ptr(),
                args.len() as i32,
            )
        }
    };
}

date_setter_thunk!(date_set_time, 0, 7);
date_setter_thunk!(date_set_full_year, 0, 0);
date_setter_thunk!(date_set_month, 0, 1);
date_setter_thunk!(date_set_date, 0, 2);
date_setter_thunk!(date_set_hours, 0, 3);
date_setter_thunk!(date_set_minutes, 0, 4);
date_setter_thunk!(date_set_seconds, 0, 5);
date_setter_thunk!(date_set_milliseconds, 0, 6);
date_setter_thunk!(date_set_year, 0, 8);
date_setter_thunk!(date_set_utc_full_year, 1, 0);
date_setter_thunk!(date_set_utc_month, 1, 1);
date_setter_thunk!(date_set_utc_date, 1, 2);
date_setter_thunk!(date_set_utc_hours, 1, 3);
date_setter_thunk!(date_set_utc_minutes, 1, 4);
date_setter_thunk!(date_set_utc_seconds, 1, 5);
date_setter_thunk!(date_set_utc_milliseconds, 1, 6);

/// `Date.prototype.toLocaleString(locales?, options?)` thunk.
///
/// The HIR instance fast path only fires for zero-arg calls (falling through to
/// `Expr::DateToLocaleString` → `js_date_to_locale_string`).  Non-zero-arg
/// calls come through the generic method-call path and land here.  Brand-check
/// `this`, then delegate to `temporal_locale_string` with `PlainDateTime`
/// defaults ({year, month, day, hour, minute, second}) and no type-specific
/// restrictions — `Date` accepts `timeZone`, `timeStyle`, `dateStyle`, etc.
extern "C" fn date_to_locale_string_opts(
    _closure: *const crate::closure::ClosureHeader,
    rest: f64,
) -> f64 {
    let this = require_date_this();
    let epoch_ms = crate::date::date_cell_timestamp(this);
    if epoch_ms.is_nan() {
        let s = crate::date::js_date_to_locale_string(this);
        return crate::value::js_nanbox_string(s as i64);
    }
    let args = super::global_this::global_this_rest_array_values(rest);
    let undef = f64::from_bits(crate::value::TAG_UNDEFINED);
    let locale = args.get(0).copied().unwrap_or(undef);
    let opts = args.get(1).copied().unwrap_or(undef);
    crate::intl::temporal_locale_string(
        epoch_ms,
        locale,
        opts,
        crate::intl::TemporalLocaleCtx::PlainDateTime,
    )
}

/// Install the brand-checked `Date.prototype` setter thunks. Called from
/// `global_this::populate_builtin_prototype_methods`'s `"Date"` arm AFTER the
/// no-op block, so these overwrite the no-op setter entries. Each is installed
/// as a variadic (`rest`) method so the optional trailing components arrive in
/// the `rest` array; `spec_length` is the ECMAScript `.length`.
pub(crate) fn install_date_proto_setters(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    // (name, func_ptr, spec `.length`)
    let methods: &[(&str, *const u8, u32)] = &[
        ("setTime", date_set_time as *const u8, 1),
        ("setFullYear", date_set_full_year as *const u8, 3),
        ("setMonth", date_set_month as *const u8, 2),
        ("setDate", date_set_date as *const u8, 1),
        ("setHours", date_set_hours as *const u8, 4),
        ("setMinutes", date_set_minutes as *const u8, 3),
        ("setSeconds", date_set_seconds as *const u8, 2),
        ("setMilliseconds", date_set_milliseconds as *const u8, 1),
        ("setYear", date_set_year as *const u8, 1),
        ("setUTCFullYear", date_set_utc_full_year as *const u8, 3),
        ("setUTCMonth", date_set_utc_month as *const u8, 2),
        ("setUTCDate", date_set_utc_date as *const u8, 1),
        ("setUTCHours", date_set_utc_hours as *const u8, 4),
        ("setUTCMinutes", date_set_utc_minutes as *const u8, 3),
        ("setUTCSeconds", date_set_utc_seconds as *const u8, 2),
        (
            "setUTCMilliseconds",
            date_set_utc_milliseconds as *const u8,
            1,
        ),
    ];
    for (name, ptr, length) in methods.iter().copied() {
        // call_fixed_arity = 0: every argument arrives in the `rest` array, so
        // one thunk shape covers the 0..=4-arg setters uniformly.
        super::global_this::install_proto_method_rest_with_length(proto_obj, name, ptr, length, 0);
    }
}

/// Overwrite the no-op `Date.prototype.toLocaleString` with a real
/// brand-checking thunk that forwards locale/options to the Intl formatter.
/// Called from `global_this::populate_builtin_prototype_methods`'s `"Date"` arm
/// after `install_noop_proto_methods`.
pub(crate) fn install_date_proto_to_locale_string(proto_obj: *mut ObjectHeader) {
    if proto_obj.is_null() {
        return;
    }
    super::global_this::install_proto_method_rest_with_length(
        proto_obj,
        "toLocaleString",
        date_to_locale_string_opts as *const u8,
        0,
        0,
    );
}
