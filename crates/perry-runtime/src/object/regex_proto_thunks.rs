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
    #[cfg(feature = "regex-engine")]
    {
        let value = crate::regex::perex_api::finish(crate::regex::perex_match_search::flags(
            crate::object::js_implicit_this_get(),
        ));
        crate::value::js_nanbox_string(value as i64)
    }
    #[cfg(not(feature = "regex-engine"))]
    {
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let re = scope.root_raw_const_ptr(re);
    let s = crate::value::js_jsvalue_to_string_coerce(arg);
    let arr = crate::regex::js_regexp_exec(re.get_raw_mut_ptr::<crate::regex::RegExpHeader>(), s);
    if arr.is_null() {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
    }
}

/// Recognize the actual builtin implementation after observable Get(exec).
/// Property names and the receiver's brand do not prove a callable is builtin.
#[cfg(feature = "regex-engine")]
pub(crate) fn is_builtin_regexp_exec(value: f64) -> bool {
    if !super::is_callable_function_value(value) {
        return false;
    }
    let closure =
        crate::value::js_nanbox_get_pointer(value) as *const crate::closure::ClosureHeader;
    crate::closure::js_closure_get_func(closure) == regex_proto_exec_thunk as *const u8
}

/// Generic `RegExp.prototype.test(string)`: require an object, ToString the
/// argument, then RegExpExec (including an overridden exec).
#[cfg(feature = "regex-engine")]
pub(super) extern "C" fn regex_proto_test_thunk(
    _c: *const crate::closure::ClosureHeader,
    arg: f64,
) -> f64 {
    let matched = crate::regex::perex_api::finish(crate::regex::perex_dispatch::test_value(
        crate::object::js_implicit_this_get(),
        arg,
    ));
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
/// NOT treat `RegExp.prototype` specially — builtin exec requires a matcher.
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

/// The complete install-time proof record for `RegExp.prototype.test`.
///
/// Keeping the three words behind one [`HotKey`](crate::tls_hot::HotKey) is
/// load-bearing on Darwin: a call resolves one TLS address, not three. The
/// first two fields are GC roots and are visited together by
/// [`scan_canonical_test_site_roots_mut`].
#[cfg(any(test, feature = "regex-engine"))]
struct CanonicalTestSite {
    /// The realm's `RegExp.prototype`, as a raw heap address.
    prototype: std::sync::atomic::AtomicI64,
    /// The canonical `test` closure, as a NaN-boxed root word.
    closure: std::sync::atomic::AtomicU64,
    /// The field index occupied by the prototype's own `test`.
    index: std::sync::atomic::AtomicU32,
}

#[cfg(any(test, feature = "regex-engine"))]
impl CanonicalTestSite {
    const EMPTY: Self = Self {
        prototype: std::sync::atomic::AtomicI64::new(0),
        closure: std::sync::atomic::AtomicU64::new(0),
        index: std::sync::atomic::AtomicU32::new(u32::MAX),
    };
}

#[cfg(any(test, feature = "regex-engine"))]
crate::perry_thread_local! {
    static REGEXP_PROTOTYPE_TEST_SITE: CanonicalTestSite = const { CanonicalTestSite::EMPTY };
}

/// FNV-1a(`"test"`) & 63 = 37. This is the exact bit
/// `descriptor_state::note_meta_descriptor_key` sets when an accessor named
/// `test` is installed. Recording it as a constant removes the four-byte hash
/// from every view-mode regex call.
#[cfg(any(test, feature = "regex-engine"))]
const TEST_ACCESSOR_KEY_BIT: u64 = 1u64 << 37;

/// Visit both pointer-bearing fields in the one per-realm record. The raw
/// prototype address and the NaN-boxed closure deliberately use different
/// visitor operations so evacuation rewrites each representation correctly.
#[cfg(feature = "regex-engine")]
pub(crate) fn scan_canonical_test_site_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    REGEXP_PROTOTYPE_TEST_SITE.with(|site| {
        visitor.visit_atomic_i64_slot(
            &site.prototype,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Release,
        );
        visitor.visit_atomic_nanbox_u64_slot(
            &site.closure,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Release,
        );
    });
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_recorded_regexp_prototype() -> i64 {
    REGEXP_PROTOTYPE_TEST_SITE
        .with(|site| site.prototype.load(std::sync::atomic::Ordering::Acquire))
}

/// How many by-name walks the canonicality proof has done in this process.
/// The fast path does none: the only walk is the one-time recording below, so
/// this must read **1 per realm**, not one per call. It is the counter that
/// says the fast path is actually the path being taken.
#[cfg(any(test, feature = "regex-engine"))]
pub(crate) static REGEXP_PROTOTYPE_TEST_WALKS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// Is `RegExp.prototype.test` still the builtin, for the regex `value`?
///
/// The `Intl.Segmenter` view mode answers `regex.test(segment)` without
/// materialising the segment, so it must not silently bypass a user
/// replacement — and it asks this question TWICE PER GRAPHEME, so the question
/// has to be answered in loads.
///
/// It used to be answered by `js_object_get_prototype_of` (the general spec
/// entry: proxy trap, Temporal cell, primitive-wrapper resolution by name) plus
/// a by-name own-field lookup that hashes `"test"` on every call. Symbolised,
/// that proof was **~13 % of the loop's thread** —
/// `get_field_by_name_object_tail` 3.6, `js_object_get_field_by_name` 3.5,
/// `get_accessor_descriptor` 2.1, `closure_get_dynamic_prop` 1.75,
/// `RandomState::hash_one<&str>` 1.4, `js_object_get_prototype_of` 1.3 —
/// against 0.8 % for the match it was guarding.
///
/// The property being tested belongs to `RegExp.prototype`, not to the call, so
/// it is recorded once at install time: the prototype pointer, the FIELD INDEX
/// its `test` occupies, and the canonical closure value. A call then reads that
/// one slot by index and compares. Everything this can get wrong, it gets wrong
/// in the declining direction:
///
/// * `test` replaced or deleted -> the slot no longer holds the recorded
///   closure -> decline;
/// * the prototype reshaped so the index means a different key -> the slot does
///   not hold the recorded closure -> decline;
/// * an accessor installed with `defineProperty(proto,"test",{get})`, which
///   leaves the old closure in the data slot -> the per-key accessor Bloom bit
///   catches it, read straight off the meta record;
/// * the receiver reparented, so the `test` it would resolve is not this one ->
///   `object_static_prototype` says a prototype was recorded -> decline.
///
/// No invalidation hook on any shared write path, which is the alternative
/// design and the one that would make every property store in the program pay
/// for this.
#[cfg(feature = "regex-engine")]
pub(crate) fn regexp_prototype_test_is_canonical(value: f64) -> bool {
    let jv_recv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv_recv.is_pointer() {
        return false;
    }
    let recv_addr = jv_recv.as_pointer::<u8>() as usize;
    if recv_addr == 0 {
        return false;
    }
    // A regex with no recorded prototype still has its class default, which is
    // the object recorded below. `object_static_prototype` answers from the
    // object's own meta record, or from an atomic "nothing was ever recorded"
    // latch — no mutex, no chain walk.
    if super::prototype_chain::object_static_prototype_known_non_meta(recv_addr).is_some() {
        return false;
    }
    REGEXP_PROTOTYPE_TEST_SITE.with(|site| {
        let proto_ptr = site.prototype.load(std::sync::atomic::Ordering::Acquire);
        let canonical = site.closure.load(std::sync::atomic::Ordering::Acquire);
        let index = site.index.load(std::sync::atomic::Ordering::Acquire);
        if proto_ptr == 0 || canonical == 0 || index == u32::MAX {
            return false;
        }
        let proto_obj = proto_ptr as *mut ObjectHeader;
        // Both reads below are of values the collector maintains: the
        // prototype address is a scanned root, and the recorded closure is a
        // scanned nanbox word, so a move rewrites both and this compare stays
        // an identity compare.
        let current = crate::object::js_object_get_field(proto_obj, index);
        if current.bits() != canonical {
            return false;
        }
        // `defineProperty(proto, "test", { get })` leaves the data slot alone
        // and records the accessor. The prototype is an ObjectHeader, so its
        // meta edge can be read directly: no cell classification and no key
        // hash on this per-call path. A null meta proves no accessor was ever
        // installed; the Bloom bit is monotonic once set.
        let meta = unsafe { (*proto_obj).meta };
        meta.is_null() || unsafe { (*meta).accessor_key_bits & TEST_ACCESSOR_KEY_BIT == 0 }
    })
}

/// Non-observable admission for a substring view. An exec/test accessor or
/// override must run once on the materialized JS argument, so never invoke
/// one while deciding whether to take this optimization.
#[cfg(feature = "regex-engine")]
pub(crate) fn regexp_view_uses_builtin(value: f64) -> bool {
    if !regexp_prototype_test_is_canonical(value) {
        return false;
    }
    let addr = crate::value::js_nanbox_get_pointer(value) as usize;
    for name in ["test", "exec"] {
        if super::exotic_expando::exotic_has_own_property(
            super::exotic_expando::ExoticKind::RegExp,
            addr,
            name,
        ) {
            return false;
        }
    }
    let proto = REGEXP_PROTOTYPE_PTR.load(std::sync::atomic::Ordering::Acquire);
    if super::descriptor_state::may_have_descriptor_entry(proto as usize, "exec", true) {
        return false;
    }
    let exec = super::js_object_get_own_field_or_undef(
        crate::value::js_nanbox_pointer(proto as i64),
        b"exec".as_ptr(),
        4,
    );
    is_builtin_regexp_exec(exec)
}

/// Record the prototype, the index of its own `test`, and the canonical
/// closure. Called once, from the installer below.
#[cfg(feature = "regex-engine")]
fn record_canonical_test_site(proto_obj: *mut ObjectHeader) {
    REGEXP_PROTOTYPE_TEST_WALKS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
    let own = super::js_object_get_own_field_or_undef(proto_value, b"test".as_ptr(), 4);
    let jv = crate::value::JSValue::from_bits(own.to_bits());
    if !jv.is_pointer() {
        return;
    }
    // The index of the KEY `"test"` in the prototype's keys array IS its field
    // index. Done once, at install, with the ordinary accessors.
    let keys = unsafe { super::object_keys_array(proto_obj) };
    if keys.is_null() {
        return;
    }
    let count = crate::array::js_array_length(keys);
    let mut found: Option<u32> = None;
    for i in 0..count {
        let key = crate::array::js_array_get_f64(keys, i);
        let matches = unsafe {
            crate::string::js_string_key_matches_bytes(
                crate::value::JSValue::from_bits(key.to_bits()),
                b"test",
            )
        };
        if matches {
            found = Some(i as u32);
            break;
        }
    }
    let Some(index) = found else {
        return;
    };
    // The recorded index must actually hold the closure we just read, or the
    // per-call load would compare the wrong slot.
    if crate::object::js_object_get_field(proto_obj, index).bits() != own.to_bits() {
        return;
    }
    REGEXP_PROTOTYPE_TEST_SITE.with(|site| {
        site.index
            .store(index, std::sync::atomic::Ordering::Release);
        // GC_STORE_AUDIT(ROOT): `site.closure` is a mutable nanbox root visited
        // by `scan_canonical_test_site_roots_mut`.
        crate::gc::runtime_store_root_atomic_nanbox_u64(
            &site.closure,
            own.to_bits(),
            std::sync::atomic::Ordering::Release,
        );
        // GC_STORE_AUDIT(ROOT): `site.prototype` is a mutable raw-address root
        // visited by `scan_canonical_test_site_roots_mut`.
        crate::gc::runtime_store_root_atomic_raw_i64(
            &site.prototype,
            proto_obj as i64,
            std::sync::atomic::Ordering::Release,
        );
    });
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
    #[cfg(feature = "regex-engine")]
    record_canonical_test_site(proto_obj);
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
    #[cfg(feature = "regex-engine")]
    install_regex_symbol_methods(proto_obj);
}

#[cfg(feature = "regex-engine")]
fn install_regex_symbol_methods(proto: *mut crate::object::ObjectHeader) {
    use crate::gc::RuntimeHandleScope;
    use crate::value::js_nanbox_pointer;
    let scope = RuntimeHandleScope::new();
    let proto = scope.root_raw_mut_ptr(proto);
    for (symbol, name, fp, arity) in [
        (
            "match",
            "[Symbol.match]",
            crate::regex::perex_match_search::match_thunk as *const u8,
            1,
        ),
        (
            "search",
            "[Symbol.search]",
            crate::regex::perex_match_search::search_thunk as *const u8,
            1,
        ),
        (
            "matchAll",
            "[Symbol.matchAll]",
            crate::regex::match_all::regexp_thunk as *const u8,
            1,
        ),
        (
            "split",
            "[Symbol.split]",
            crate::regex::perex_split::regexp_thunk as *const u8,
            2,
        ),
        (
            "replace",
            "[Symbol.replace]",
            crate::regex::perex_replace::regexp_thunk as *const u8,
            2,
        ),
    ] {
        let iteration = RuntimeHandleScope::new();
        crate::closure::js_register_closure_arity(fp, arity);
        let function = iteration.root_raw_mut_ptr(crate::closure::js_closure_alloc(fp, 0));
        super::native_module::set_bound_native_closure_name(function.get_raw_mut_ptr(), name);
        super::native_module::set_builtin_closure_length(
            function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as usize,
            arity,
        );
        let key = iteration.root_raw_mut_ptr(crate::symbol::well_known_symbol(symbol));
        unsafe {
            crate::symbol::js_object_set_symbol_property(
                js_nanbox_pointer(proto.get_raw_mut_ptr::<crate::object::ObjectHeader>() as i64),
                js_nanbox_pointer(key.get_raw_mut_ptr::<crate::symbol::SymbolHeader>() as i64),
                js_nanbox_pointer(
                    function.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64
                ),
            );
        }
        crate::symbol::set_symbol_property_attrs(
            proto.get_raw_mut_ptr::<crate::object::ObjectHeader>() as usize,
            key.get_raw_mut_ptr::<crate::symbol::SymbolHeader>() as usize,
            crate::object::PropertyAttrs::new(true, false, true),
        );
    }
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

/// RegExp owns lastIndex; source/flags/boolean getters live on its prototype.
/// Keep property-key ownership and all GC roots outside callback traps.
pub(crate) fn regexp_get_property(
    receiver: *const ObjectHeader,
    key: *const crate::StringHeader,
) -> crate::JSValue {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(receiver as i64));
    let key = scope.root_string_ptr(key);
    let name = unsafe {
        key.with_string_bytes(|bytes| std::str::from_utf8(bytes).ok().map(str::to_owned))
    };
    if name.as_deref() == Some("lastIndex") {
        let re = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64())
            as *const crate::regex::RegExpHeader;
        return crate::JSValue::from_bits(crate::regex::js_regexp_get_last_index(re).to_bits());
    }
    let previous = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let result = crate::exception::catch_js_throw(|| {
        if let Some(name) = &name {
            let addr = crate::value::js_nanbox_get_pointer(receiver.get_nanbox_f64()) as usize;
            // The caller classified this receiver as a live RegExp, and the
            // root above keeps it current across an own-property getter.
            if let Some(value) = unsafe {
                super::exotic_expando::exotic_get_own_property(
                    addr,
                    super::exotic_expando::ExoticKind::RegExp,
                    name,
                    receiver.get_nanbox_f64(),
                )
            } {
                return value;
            }
        }
        let proto = scope.root_nanbox_f64(crate::object::js_object_get_prototype_of(
            receiver.get_nanbox_f64(),
        ));
        if !crate::proxy::reflect_value_is_object(proto.get_nanbox_f64()) {
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        crate::proxy::js_reflect_get(
            proto.get_nanbox_f64(),
            crate::value::js_nanbox_string(key.get_raw_const_ptr::<crate::StringHeader>() as i64),
            receiver.get_nanbox_f64(),
        )
    });
    crate::object::js_implicit_this_set(previous.get_nanbox_f64());
    drop(name);
    match result {
        Ok(value) => crate::JSValue::from_bits(value.to_bits()),
        Err(error) => crate::exception::js_throw(error),
    }
}
