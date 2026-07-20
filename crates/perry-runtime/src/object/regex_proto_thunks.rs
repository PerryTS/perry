//! RegExp.prototype accessor getters (`source`, `flags`, `global`,
//! `ignoreCase`, `multiline`, `dotAll`, `sticky`, `unicode`, `unicodeSets`,
//! `hasIndices`).
//!
//! Per ECMA-262 these are *accessor* properties living on `RegExp.prototype`
//! with a native getter function and `set: undefined`, `enumerable: false`,
//! `configurable: true`. Instances expose them through the prototype chain, so
//! `re.global` works via the inherited getter and reflection
//! (`Object.getOwnPropertyDescriptor(RegExp.prototype, "global").get`) finds the
//! real getter. Each getter brand-checks `this`:
//!
//!   * a real RegExp instance → read the flag/source,
//!   * exactly `RegExp.prototype` → return the spec sentinel (`undefined` for
//!     the boolean flags, `"(?:)"` for `source`, `""` for `flags`),
//!   * anything else → `TypeError`.
//!
//! The `flags` getter is special: per spec it does NOT brand-check
//! `[[OriginalFlags]]`; it reads `hasIndices`/`global`/… off the (generic)
//! receiver via `Get` + `ToBoolean` and assembles the string. So
//! `RegExp.prototype.flags.call({ global: 1, … })` works on a plain object.
//!
//! Installed onto `RegExp.prototype` by
//! `global_this::populate_builtin_prototype_methods`.

use super::*;

/// Result of resolving the `this` receiver for a brand-checked flag/source
/// getter.
enum RegexReceiver {
    /// A live RegExp instance.
    Regex(*const crate::regex::RegExpHeader),
    /// Exactly `RegExp.prototype` — getters return the spec sentinel.
    Prototype,
}

/// Resolve `IMPLICIT_THIS` to a RegExp instance or `RegExp.prototype`, throwing
/// `TypeError` otherwise (matching the spec brand check shared by every
/// flag/`source` getter).
fn regex_receiver_or_throw(getter: &str) -> RegexReceiver {
    let receiver = crate::value::JSValue::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if receiver.is_pointer() {
        let ptr = receiver.as_pointer::<u8>() as usize;
        if crate::regex::is_registered_regex(ptr) {
            return RegexReceiver::Regex(ptr as *const crate::regex::RegExpHeader);
        }
        let proto = crate::value::JSValue::from_bits(
            super::global_this::builtin_prototype_value("RegExp").to_bits(),
        );
        if proto.is_pointer() && proto.as_pointer::<u8>() as usize == ptr {
            return RegexReceiver::Prototype;
        }
    }
    throw_regex_brand_error(getter)
}

fn throw_regex_brand_error(getter: &str) -> ! {
    let msg = format!("get RegExp.prototype.{getter} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Does the regex's (canonical) flags string contain `flag`?
fn regex_has_flag(re: *const crate::regex::RegExpHeader, flag: char) -> bool {
    let s = crate::regex::js_regexp_get_flags(re);
    if s.is_null() {
        return false;
    }
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        bytes.iter().any(|&b| b as char == flag)
    }
}

/// Shared body for the boolean flag getters: regex → boolean, prototype →
/// undefined, else TypeError.
fn flag_getter(getter: &str, flag: char) -> f64 {
    match regex_receiver_or_throw(getter) {
        RegexReceiver::Regex(re) => {
            f64::from_bits(crate::value::JSValue::bool(regex_has_flag(re, flag)).bits())
        }
        RegexReceiver::Prototype => f64::from_bits(crate::value::TAG_UNDEFINED),
    }
}

pub(super) extern "C" fn regex_proto_global_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("global", 'g')
}
pub(super) extern "C" fn regex_proto_ignore_case_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("ignoreCase", 'i')
}
pub(super) extern "C" fn regex_proto_multiline_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("multiline", 'm')
}
pub(super) extern "C" fn regex_proto_dot_all_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("dotAll", 's')
}
pub(super) extern "C" fn regex_proto_sticky_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("sticky", 'y')
}
pub(super) extern "C" fn regex_proto_unicode_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("unicode", 'u')
}
pub(super) extern "C" fn regex_proto_unicode_sets_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("unicodeSets", 'v')
}
pub(super) extern "C" fn regex_proto_has_indices_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    flag_getter("hasIndices", 'd')
}

pub(super) extern "C" fn regex_proto_source_getter(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    match regex_receiver_or_throw("source") {
        RegexReceiver::Regex(re) => {
            // `js_regexp_get_source` returns the *escaped* source string.
            let s = crate::regex::js_regexp_get_source(re);
            f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
        }
        RegexReceiver::Prototype => {
            let s = crate::regex::js_regexp_empty_source();
            f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
        }
    }
}

/// `get RegExp.prototype.flags` — spec 22.2.6.4. Reads each flag property off
/// the (generic) receiver via `Get` + `ToBoolean` and assembles in canonical
/// order `d g i m s u v y`. Throws `TypeError` only if `this` is not an Object.
pub(super) extern "C" fn regex_proto_flags_getter(_c: *const crate::closure::ClosureHeader) -> f64 {
    let receiver = crate::value::JSValue::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    // Type(R) must be Object. Pointer-tagged values are objects EXCEPT Symbols
    // (which are also pointer-tagged via the symbol side-table); a Symbol `this`
    // must throw a TypeError, not silently assemble "".
    if !receiver.is_pointer()
        || crate::symbol::is_registered_symbol(receiver.as_pointer::<u8>() as usize)
    {
        throw_regex_brand_error("flags");
    }
    let recv_bits = receiver.bits();
    let recv_f64 = f64::from_bits(recv_bits);
    let mut out = String::with_capacity(8);
    for (name, ch) in [
        ("hasIndices", 'd'),
        ("global", 'g'),
        ("ignoreCase", 'i'),
        ("multiline", 'm'),
        ("dotAll", 's'),
        ("unicode", 'u'),
        ("unicodeSets", 'v'),
        ("sticky", 'y'),
    ] {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_val = crate::js_nanbox_string(key as i64);
        let v = crate::value::js_dyn_index_get(recv_f64, key_val);
        if crate::value::js_is_truthy(v) != 0 {
            out.push(ch);
        }
    }
    let s = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
    f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
}

/// Install one accessor getter (`set: undefined`) onto `proto_obj` with the
/// spec attributes (`enumerable: false`, `configurable: true`) and the proper
/// getter `name` (`"get <prop>"`) / `length` (`0`).
fn install_getter(proto_obj: *mut ObjectHeader, name: &str, func_ptr: *const u8) {
    if proto_obj.is_null() {
        return;
    }
    unsafe {
        crate::closure::js_register_closure_arity(func_ptr, 0);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        if closure.is_null() {
            return;
        }
        super::native_module::set_bound_native_closure_name(closure, &format!("get {name}"));
        super::native_module::set_builtin_closure_length(closure as usize, 0);
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        super::object_ops::ensure_key_in_keys_array(proto_obj, key);
        let getter_bits = crate::value::js_nanbox_pointer(closure as i64).to_bits();
        super::object_ops::install_builtin_getter(proto_obj, name, getter_bits);
        super::set_accessor_descriptor(
            proto_obj as usize,
            name.to_string(),
            super::AccessorDescriptor {
                get: getter_bits,
                set: 0,
            },
        );
        super::set_property_attrs(
            proto_obj as usize,
            name.to_string(),
            super::PropertyAttrs::new(true, false, true),
        );
        super::set_builtin_property_attrs(
            closure as usize,
            "name".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
        super::set_builtin_property_attrs(
            closure as usize,
            "length".to_string(),
            super::PropertyAttrs::new(false, false, true),
        );
    }
}

/// `RegExp.prototype.exec(string)` — brand-checks `this` has `[[RegExpMatcher]]`
/// (a registered RegExp; `RegExp.prototype` itself throws), `ToString`s the
/// argument, and runs the match. Reflective: `RegExp.prototype.exec.call(re, s)`
/// and `re.exec(s)` extracted off the prototype both route here.
#[cfg(feature = "regex-engine")]
pub(super) extern "C" fn regex_proto_exec_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    let re = regex_instance_or_throw("exec");
    let s = crate::value::js_jsvalue_to_string_coerce(arg);
    let arr = crate::regex::js_regexp_exec(re as *mut crate::regex::RegExpHeader, s);
    if arr.is_null() {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
    }
}

/// `RegExp.prototype.test(string)` — brand-checks `this`, `ToString`s the arg,
/// returns a boolean.
#[cfg(feature = "regex-engine")]
pub(super) extern "C" fn regex_proto_test_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    let re = regex_instance_or_throw("test");
    let s = crate::value::js_jsvalue_to_string_coerce(arg);
    let matched = crate::regex::js_regexp_test(re as *const crate::regex::RegExpHeader, s) != 0;
    f64::from_bits(crate::value::JSValue::bool(matched).bits())
}

/// `RegExp.prototype.compile(pattern, flags)` (Annex B §B.2.5.1) — brand-checks
/// `this` has a `[[RegExpMatcher]]` internal slot (a registered RegExp; a
/// non-Object or non-RegExp receiver throws `TypeError`), then re-initializes
/// the receiver in place. Reflective: `RegExp.prototype.compile.call(re, p, f)`
/// and the extracted-then-called form both route here; a direct `re.compile(p,
/// f)` is fast-pathed in `native_call_method` but ends in the same
/// `js_regexp_compile_value`.
#[cfg(feature = "regex-engine")]
pub(super) extern "C" fn regex_proto_compile_thunk(
    _c: *const crate::closure::ClosureHeader,
    pattern: f64,
    flags: f64,
) -> f64 {
    let re = regex_instance_or_throw("compile");
    crate::regex::js_regexp_compile_value(re as *mut crate::regex::RegExpHeader, pattern, flags)
}

/// `RegExp.prototype.toString()` — per spec reads `source`/`flags` off the
/// (generic) receiver via `Get` and returns `/source/flags`. Throws `TypeError`
/// only when `this` is not an Object, so
/// `RegExp.prototype.toString.call({ source: "x", flags: "g" })` works.
pub(super) extern "C" fn regex_proto_to_string_thunk(
    _c: *const crate::closure::ClosureHeader,
) -> f64 {
    let receiver = crate::value::JSValue::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if !receiver.is_pointer()
        || crate::symbol::is_registered_symbol(receiver.as_pointer::<u8>() as usize)
    {
        throw_regex_brand_error("toString");
    }
    let recv_f64 = f64::from_bits(receiver.bits());
    let read = |name: &str| -> String {
        let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let key_val = crate::js_nanbox_string(key as i64);
        let v = crate::value::js_dyn_index_get(recv_f64, key_val);
        let s = crate::value::js_jsvalue_to_string_coerce(v);
        if s.is_null() {
            String::new()
        } else {
            unsafe {
                let len = (*s).byte_len as usize;
                let data = (s as *const u8).add(std::mem::size_of::<crate::StringHeader>());
                std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len)).to_string()
            }
        }
    };
    let out = format!("/{}/{}", read("source"), read("flags"));
    let s = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
    f64::from_bits(crate::js_nanbox_string(s as i64).to_bits())
}

/// Resolve `IMPLICIT_THIS` to a live RegExp instance (with `[[RegExpMatcher]]`),
/// throwing `TypeError` otherwise. Unlike the flag/`source` getters, this does
/// NOT treat `RegExp.prototype` specially — `exec`/`test` require a real matcher.
#[cfg(feature = "regex-engine")]
fn regex_instance_or_throw(method: &str) -> *const crate::regex::RegExpHeader {
    let receiver = crate::value::JSValue::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if receiver.is_pointer() {
        let ptr = receiver.as_pointer::<u8>() as usize;
        if crate::regex::is_registered_regex(ptr) {
            return ptr as *const crate::regex::RegExpHeader;
        }
    }
    let msg = format!("RegExp.prototype.{method} called on incompatible receiver");
    let s = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(s);
    crate::exception::js_throw(f64::from_bits(
        crate::value::JSValue::pointer(err as *const u8).bits(),
    ))
}

/// Install the real (brand-checking) `exec`/`test`/`toString`/`compile`
/// prototype methods. `compile` is only installed here when the `regex-engine`
/// feature is on; the fallback no-op (for builds without an engine) is installed
/// by the caller.
pub(super) fn install_regex_proto_methods(proto_obj: *mut ObjectHeader) {
    use super::global_this::install_proto_method as ipm;
    #[cfg(feature = "regex-engine")]
    ipm(proto_obj, "exec", regex_proto_exec_thunk as *const u8, 1);
    #[cfg(feature = "regex-engine")]
    ipm(proto_obj, "test", regex_proto_test_thunk as *const u8, 1);
    // Annex B `compile` re-initializes the receiver in place. It needs a real
    // brand check so `RegExp.prototype.compile.call(non-regexp)` throws a
    // `TypeError` (test262 annexB `.../compile/this-{not-object,obj-not-regexp}`).
    #[cfg(feature = "regex-engine")]
    ipm(
        proto_obj,
        "compile",
        regex_proto_compile_thunk as *const u8,
        2,
    );
    ipm(
        proto_obj,
        "toString",
        regex_proto_to_string_thunk as *const u8,
        0,
    );
}

/// Install all RegExp.prototype accessor getters.
pub(super) fn install_regex_proto_accessors(proto_obj: *mut ObjectHeader) {
    install_getter(proto_obj, "flags", regex_proto_flags_getter as *const u8);
    install_getter(proto_obj, "source", regex_proto_source_getter as *const u8);
    install_getter(proto_obj, "global", regex_proto_global_getter as *const u8);
    install_getter(
        proto_obj,
        "ignoreCase",
        regex_proto_ignore_case_getter as *const u8,
    );
    install_getter(
        proto_obj,
        "multiline",
        regex_proto_multiline_getter as *const u8,
    );
    install_getter(proto_obj, "dotAll", regex_proto_dot_all_getter as *const u8);
    install_getter(proto_obj, "sticky", regex_proto_sticky_getter as *const u8);
    install_getter(
        proto_obj,
        "unicode",
        regex_proto_unicode_getter as *const u8,
    );
    install_getter(
        proto_obj,
        "unicodeSets",
        regex_proto_unicode_sets_getter as *const u8,
    );
    install_getter(
        proto_obj,
        "hasIndices",
        regex_proto_has_indices_getter as *const u8,
    );
}
