//! Value formatting for `console.log`/`util.inspect`-style output.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).
//! Contains the shared circular-reference tracking, inspect-depth/showHidden
//! guards, function-name registry, `format_jsvalue` / `format_jsvalue_for_json`
//! recursion, plus `util.format` / `util.inspect` entry points.

#[cfg(feature = "ohos-napi")]
use super::println;
use super::*;

mod array_buffer;
mod boxed_primitives;
mod collection_equality;
pub(crate) use boxed_primitives::{
    boxed_primitive_json_value, boxed_primitive_payload, boxed_primitive_to_string_tag,
};
pub use boxed_primitives::{
    js_boxed_bigint_new, js_boxed_boolean_new, js_boxed_number_new, js_boxed_string_new,
    js_boxed_symbol_new, scan_boxed_primitive_payload_roots_mut,
};
mod collections;
mod identity_equality;
mod prototype_equality;
mod strip_vt;
mod typed_array_equality;
mod util_format;

pub use strip_vt::js_util_strip_vt_control_characters;
pub use util_format::{js_util_format, js_util_format_with_options, js_util_inspect};

/// Returns true if the f64 value is negative zero (-0.0).
/// Uses bit pattern comparison so +0.0 and -0.0 are distinguished
/// (they compare equal with normal `==`).
#[inline]
pub(crate) fn is_negative_zero(n: f64) -> bool {
    n.to_bits() == 0x8000_0000_0000_0000u64
}

// Circular-reference tracking for `format_jsvalue` / `format_jsvalue_for_json`.
// Node's `util.inspect` detects cycles and prints `<ref *N>` at the head of
// the cycle plus `[Circular *N]` at the back-edge. We track:
//   - `stack`: pointer addresses currently mid-format (the ancestor chain)
//   - `ids`: pointer address → assigned ref ID (only populated for cyclic refs)
//   - `next_id`: monotonic ID counter, allocated lazily on first back-edge
// Reset at every top-level `format_jsvalue(_, 0)` call so each print starts
// fresh. See #1204.
#[derive(Default)]
struct CircularState {
    stack: Vec<usize>,
    ids: std::collections::HashMap<usize, usize>,
    next_id: usize,
}

impl CircularState {
    fn reset(&mut self) {
        self.stack.clear();
        self.ids.clear();
        self.next_id = 0;
    }
}

thread_local! {
    static INSPECT_CIRCULAR: std::cell::RefCell<CircularState> =
        std::cell::RefCell::new(CircularState::default());
}

/// Enter an object/array for formatting. Returns:
/// - `Err(id)` if `ptr_addr` is already on the ancestor stack — caller should
///   return `[Circular *id]` immediately (no push, no body).
/// - `Ok(())` after pushing `ptr_addr` — caller must call
///   `inspect_finish_circular(ptr_addr, body)` to pop + maybe prepend `<ref *N>`.
fn inspect_enter_circular(ptr_addr: usize) -> Result<(), usize> {
    INSPECT_CIRCULAR.with(|c| {
        let mut st = c.borrow_mut();
        if st.stack.contains(&ptr_addr) {
            if let Some(&id) = st.ids.get(&ptr_addr) {
                return Err(id);
            }
            st.next_id += 1;
            let id = st.next_id;
            st.ids.insert(ptr_addr, id);
            return Err(id);
        }
        st.stack.push(ptr_addr);
        Ok(())
    })
}

/// Pop `ptr_addr` from the ancestor stack and prepend `<ref *N> ` if a
/// back-edge to it was discovered during body formatting.
fn inspect_finish_circular(ptr_addr: usize, body: String) -> String {
    INSPECT_CIRCULAR.with(|c| {
        let mut st = c.borrow_mut();
        st.stack.pop();
        match st.ids.get(&ptr_addr).copied() {
            Some(id) => format!("<ref *{}> {}", id, body),
            None => body,
        }
    })
}

/// Format a finite, non-zero, non-integer-like f64 per ECMAScript
/// NumberToString. Caller has already filtered NaN / ±Infinity / ±0 /
/// integer-shaped values; this only decides decimal vs scientific
/// notation per the |n| < 10^-6 / |n| >= 10^21 thresholds.
///
/// Without the threshold split, Rust's Display impl produces 300-digit
/// decimals for `Number.MAX_VALUE` (`1.7976931348623157e+308` → 309
/// zeros) and 16-digit `0.000…0002…` decimals for `Number.EPSILON`,
/// neither of which matches Node.
#[inline]
pub(crate) fn format_finite_number_js(value: f64) -> String {
    let abs = value.abs();
    if !(1e-6..1e21).contains(&abs) {
        crate::string::fix_exponent_format(&format!("{:e}", value))
    } else {
        format!("{}", value)
    }
}

/// 2^53 — the largest magnitude below which every integer is exactly
/// representable as an `f64`. `console.log` / inspect / `util.format('%d')` fast
/// paths print a whole number via a direct `as i64` cast for speed, but that is
/// only the shortest-round-trip decimal *below* this bound. At/above it several
/// integers map to one double, and the exact integer carries more significant
/// digits than V8's shortest decimal (`2**58` → `288230376151711740`, not
/// `…744`); such values must go through `format_finite_number_js`, whose Rust
/// `{}` is shortest-round-trip. Refs #6127.
pub(crate) const INT_EXACT_FASTPATH_LIMIT: f64 = 9_007_199_254_740_992.0;

/// Format a finite, integer-valued `f64` as V8 would: the fast `as i64` cast
/// below 2^53, else the shortest-round-trip formatter. Callers that have already
/// established `value.fract() == 0.0` (or truncated) use this to avoid the exact
/// vs. shortest divergence at large magnitudes. Refs #6127.
pub(crate) fn format_integral_f64(value: f64) -> String {
    if value.abs() < INT_EXACT_FASTPATH_LIMIT {
        (value as i64).to_string()
    } else {
        format_finite_number_js(value)
    }
}

fn format_util_number(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_string()
    } else if value.is_infinite() {
        if value.is_sign_negative() {
            "-Infinity".to_string()
        } else {
            "Infinity".to_string()
        }
    } else if value == 0.0 {
        if value.is_sign_negative() {
            "-0".to_string()
        } else {
            "0".to_string()
        }
    } else {
        format_finite_number_js(value)
    }
}

/// Decode the textual content of any string-shaped JSValue (heap
/// `STRING_TAG` or inline `SHORT_STRING_TAG`) into a fresh `String`.
/// Returns `None` for non-string values. SSO values are decoded
/// inline via the value's NaN-box payload — no heap touch.
///
/// Centralizes the SSO-aware dispatch every print/format/coerce
/// path needs: pre-SSO (≤ v0.5.215), the `is_string()` check used
/// throughout this file rejected SSO so any short string returned
/// by `JSON.parse` (e.g. `"perry"` from `{"foo":"perry"}`) fell
/// through to the "regular number" branch and printed as `NaN`
/// (because SHORT_STRING_TAG bits are NaN bits).
pub(crate) fn jsvalue_string_content(value: f64) -> Option<String> {
    let mut scratch = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let (ptr, len) = crate::string::str_bytes_from_jsvalue(value, &mut scratch)?;
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        Some(
            std::str::from_utf8(bytes)
                .unwrap_or("[invalid utf8]")
                .to_string(),
        )
    }
}
/// Format a BigInt JSValue as its Node literal form (digits + `n`),
/// e.g. `5n`, `-12345678901234567890n`. Returns `"0n"` on a null ptr
/// rather than panicking so the formatter stays infallible.
pub(crate) fn format_bigint_literal(val: f64) -> String {
    use crate::value::JSValue;
    let jv = JSValue::from_bits(val.to_bits());
    let ptr = jv.as_bigint_ptr();
    if ptr.is_null() {
        return "0n".to_string();
    }
    unsafe {
        let str_ptr = crate::bigint::js_bigint_to_string(ptr);
        if str_ptr.is_null() {
            return "0n".to_string();
        }
        let len = (*str_ptr).byte_len as usize;
        let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        let num_str = std::str::from_utf8(bytes).unwrap_or("0");
        format!("{}n", num_str)
    }
}

/// Per-thread override for the depth at which nested objects/arrays
/// collapse to `[Object]` / `[Array]`. Defaults to Node's `util.inspect`
/// default of 2. The `%o` format specifier raises this temporarily to
/// Node's object-format depth of 4, while `%O` uses the current inspect
/// options unchanged.
thread_local! {
    static INSPECT_DEPTH_LIMIT: std::cell::Cell<usize> = const { std::cell::Cell::new(2) };
}

pub(crate) fn inspect_depth_limit() -> usize {
    INSPECT_DEPTH_LIMIT.with(|c| c.get())
}

/// RAII guard that sets the per-thread inspect depth limit for the
/// lifetime of the guard and restores the previous value on drop.
pub(crate) struct InspectDepthLimitGuard(usize);

impl InspectDepthLimitGuard {
    pub(crate) fn new(limit: usize) -> Self {
        let prev = INSPECT_DEPTH_LIMIT.with(|c| c.replace(limit));
        Self(prev)
    }
}

impl Drop for InspectDepthLimitGuard {
    fn drop(&mut self) {
        INSPECT_DEPTH_LIMIT.with(|c| c.set(self.0));
    }
}

/// Sidecar registry mapping each user-defined function's compiled address
/// to the JS name it should print as via `console.log` / `util.inspect`.
/// Codegen emits a `js_register_function_name(func_ptr, name_bytes, len)`
/// call from `main()` for every named function in `Hir.functions`, so by
/// the time user code runs the map is fully populated. Functions never
/// rename, so we accept lossy single-writer semantics (last-write wins on
/// the rare duplicate). See #1202.
///
/// Direct lookup against the symbol table via `dladdr` doesn't work here
/// because the macOS linker's `-dead_strip` removes the symbol *names* of
/// perry_fn_* globals (the bodies stay — they're referenced by pointer —
/// but the symbol entries vanish, so `dli_sname` comes back null).
fn function_name_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<str>>> {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<
        std::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<str>>>,
    > = OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Format a JS function/closure for `console.log` / `util.inspect`. Returns
/// `[Function: <name>]` when codegen has registered a name for the
/// function pointer, otherwise `[Function (anonymous)]` (matching Node's
/// output for nameless closures). When the closure carries user-attached
/// own properties (`func.toString = …`, `func.x = 1`, etc.), append a
/// Node-style `{ key: value, … }` listing — without invoking any user
/// coercion hook. Built-in slots (`name`, `prototype`, `length`,
/// `arguments`, `caller`) are filtered out so the output matches Node's
/// `util.inspect` for `function f() {}; console.log(f)` (no decoration)
/// vs. `f.x = 1; console.log(f)` (decorated). See #1202 / #1203.
fn format_function_for_console(closure_ptr: *const crate::closure::ClosureHeader) -> String {
    if closure_ptr.is_null() {
        return "[Function (anonymous)]".to_string();
    }

    // Snapshot user-attached own properties and filter out the built-in
    // function slots that Node hides from `util.inspect`. Node prints
    // these only when the user reassigned them — but `prototype` and
    // `name` are runtime-allocated on every function, so always hiding
    // them yields parity for the common case (`f.x = 1`).
    let props = crate::closure::closure_dynamic_props_snapshot(closure_ptr as usize);

    // The function name: prefer the codegen-registered name keyed by the
    // function pointer (real named declarations), then fall back to the
    // closure's own `name` property. Bound native-module exports
    // (`child_process.ChildProcess`, `tty.ReadStream`, `events.EventEmitter`,
    // …) all share one `BOUND_METHOD_FUNC_PTR`, so they carry no per-pointer
    // registry name — their identity lives in the `name` prop set by
    // `set_bound_native_closure_name`. Reading it here makes them print
    // `[Function: ChildProcess]` instead of `[Function (anonymous)]`,
    // matching Node. #1856.
    let registry_name: Option<String> = unsafe {
        let func_ptr = (*closure_ptr).func_ptr;
        if func_ptr.is_null() {
            None
        } else {
            function_name_registry()
                .lock()
                .ok()
                .and_then(|map| map.get(&(func_ptr as usize)).map(|n| n.to_string()))
                .filter(|n| !n.is_empty())
        }
    };
    let label = match registry_name.or_else(|| {
        props
            .iter()
            .find(|(k, _)| k == "name")
            .and_then(|(_, v)| jsvalue_string_content(*v))
            .filter(|n| !n.is_empty())
    }) {
        Some(name) => format!("[Function: {name}]"),
        None => "[Function (anonymous)]".to_string(),
    };

    let user_props: Vec<(String, f64)> = props
        .into_iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "name" | "prototype" | "length" | "arguments" | "caller"
            )
        })
        .collect();
    if user_props.is_empty() {
        return label;
    }
    let mut parts: Vec<String> = Vec::with_capacity(user_props.len());
    for (k, v) in user_props {
        // `format_jsvalue` skips toString/Symbol.toPrimitive coercion
        // hooks — exactly what #1203 needs (Node MUST NOT call the
        // user's `toString` while inspecting).
        parts.push(format!("{}: {}", k, format_jsvalue(v, 1)));
    }
    format!("{} {{ {} }}", label, parts.join(", "))
}

/// Codegen-facing entry point: register `func_ptr` as the compiled address
/// of a JS function called `<name>` (UTF-8, `name_len` bytes, not NUL-
/// terminated). Idempotent — calling twice with the same `func_ptr`
/// silently overwrites the prior name.
///
/// # Safety
///
/// `name_ptr..name_ptr+name_len` must point at a valid UTF-8 byte slice
/// that outlives the call (we copy it). `func_ptr` may be anything; we
/// only use it as a map key.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_name(
    func_ptr: *const u8,
    name_ptr: *const u8,
    name_len: u32,
) {
    if func_ptr.is_null() || name_ptr.is_null() || name_len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(name_ptr, name_len as usize);
    let Ok(name) = std::str::from_utf8(bytes) else {
        return;
    };
    if let Ok(mut map) = function_name_registry().lock() {
        map.insert(func_ptr as usize, std::sync::Arc::from(name));
    }
}

/// Register `name` for `func_ptr` only if no name was previously registered.
/// Used by computed-key object literal assignment: when `{ [sym]: fn }` is
/// stored, Node infers the function's name from the symbol's description
/// (`[Function: [<desc>]]`). Anonymous closures hit this; closures that
/// already have a real name (`function f(){}`) are left alone.
///
/// Safe to call from any runtime path — uses the same mutex as
/// `js_register_function_name`.
pub fn register_function_name_if_absent(func_ptr: usize, name: &str) {
    if func_ptr == 0 || name.is_empty() {
        return;
    }
    if let Ok(mut map) = function_name_registry().lock() {
        map.entry(func_ptr)
            .or_insert_with(|| std::sync::Arc::from(name));
    }
}

/// Look up the codegen-registered JS name for a function pointer.
///
/// Returns the name registered by `js_register_function_name` (keyed on the
/// `__perry_wrap_<name>` wrapper address that `js_closure_alloc_singleton`
/// stamps into the `ClosureHeader`), or `None` when no non-empty name was
/// registered. Used by the spec `fn.name` own-property read (#2059) and by
/// `getOwnPropertyDescriptor(fn, "name")` — the same registry the
/// `[Function: <name>]` console formatter already consults.
pub fn function_name_for_ptr(func_ptr: usize) -> Option<String> {
    if func_ptr == 0 {
        return None;
    }
    function_name_registry()
        .lock()
        .ok()
        .and_then(|map| map.get(&func_ptr).map(|n| n.to_string()))
        .filter(|n| !n.is_empty())
}

/// #4101: sidecar registry mapping each user function's compiled address to
/// its retained original source text. Populated by `js_register_function_source`
/// (emitted from module init alongside `js_register_function_name`), so by the
/// time user code runs the map is fully populated. Mirrors the function-name
/// registry's single-writer, last-write-wins semantics.
fn function_source_registry(
) -> &'static std::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<str>>> {
    use std::sync::OnceLock;
    static REGISTRY: OnceLock<
        std::sync::Mutex<std::collections::HashMap<usize, std::sync::Arc<str>>>,
    > = OnceLock::new();
    REGISTRY.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Codegen-facing entry point: register `func_ptr` as the compiled address of
/// a JS function whose original source spans `src_ptr..src_ptr+src_len` (UTF-8,
/// not NUL-terminated). Idempotent — last write wins.
///
/// # Safety
///
/// `src_ptr..src_ptr+src_len` must point at a valid UTF-8 byte slice that
/// outlives the call (we copy it). `func_ptr` is used only as a map key.
#[no_mangle]
pub unsafe extern "C" fn js_register_function_source(
    func_ptr: *const u8,
    src_ptr: *const u8,
    src_len: u32,
) {
    if func_ptr.is_null() || src_ptr.is_null() || src_len == 0 {
        return;
    }
    let bytes = std::slice::from_raw_parts(src_ptr, src_len as usize);
    let Ok(src) = std::str::from_utf8(bytes) else {
        return;
    };
    if let Ok(mut map) = function_source_registry().lock() {
        map.insert(func_ptr as usize, std::sync::Arc::from(src));
    }
}

/// Look up the codegen-registered source text for a function pointer.
pub fn function_source_for_ptr(func_ptr: usize) -> Option<String> {
    if func_ptr == 0 {
        return None;
    }
    function_source_registry()
        .lock()
        .ok()
        .and_then(|map| map.get(&func_ptr).map(|s| s.to_string()))
        .filter(|s| !s.is_empty())
}

/// #4101: build the `Function.prototype.toString` result for a closure whose
/// `ClosureHeader.func_ptr` is `func_ptr`. Returns the retained source text
/// when codegen registered it, otherwise a synthesized native form
/// (`function <name>() { [native code] }`) matching Node's output for
/// functions without recoverable source (built-ins / bound natives).
pub fn function_source_for_func_ptr(func_ptr: usize) -> String {
    if let Some(src) = function_source_for_ptr(func_ptr) {
        return src;
    }
    let name = function_name_for_ptr(func_ptr).unwrap_or_default();
    format!("function {name}() {{ [native code] }}")
}

/// Per-thread override for the `showHidden` inspect option. Defaults to
/// `false` (Node default): `util.inspect` / `console.log` only show
/// enumerable properties. `console.dir(value, { showHidden: true })`
/// flips this for the duration of the print so non-enumerable props
/// surface in `[bracketed]` form. See #1200.
thread_local! {
    static INSPECT_SHOW_HIDDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INSPECT_SHOW_PROXY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn inspect_show_hidden() -> bool {
    INSPECT_SHOW_HIDDEN.with(|c| c.get())
}

fn inspect_show_proxy() -> bool {
    INSPECT_SHOW_PROXY.with(|c| c.get())
}

/// RAII guard for `INSPECT_SHOW_HIDDEN`; restores the previous value on
/// drop so nested format calls don't leak the override.
pub(crate) struct InspectShowHiddenGuard(bool);

impl InspectShowHiddenGuard {
    pub(crate) fn new(show: bool) -> Self {
        let prev = INSPECT_SHOW_HIDDEN.with(|c| c.replace(show));
        Self(prev)
    }
}

impl Drop for InspectShowHiddenGuard {
    fn drop(&mut self) {
        INSPECT_SHOW_HIDDEN.with(|c| c.set(self.0));
    }
}

pub(crate) struct InspectShowProxyGuard(bool);

impl InspectShowProxyGuard {
    pub(crate) fn new(show: bool) -> Self {
        let prev = INSPECT_SHOW_PROXY.with(|c| c.replace(show));
        Self(prev)
    }
}

impl Drop for InspectShowProxyGuard {
    fn drop(&mut self) {
        INSPECT_SHOW_PROXY.with(|c| c.set(self.0));
    }
}

/// Per-thread override for the `customInspect` inspect option. Defaults to
/// `true` (Node default for `util.inspect` / `console.log`): when an object
/// has a `[util.inspect.custom]` symbol-keyed method, the hook is invoked
/// and its return value replaces the default object body. `console.dir`
/// flips this to `false` so the symbol surfaces as a property listing.
/// See #1201.
thread_local! {
    static INSPECT_CUSTOM_INSPECT: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(crate) fn inspect_custom_inspect_enabled() -> bool {
    INSPECT_CUSTOM_INSPECT.with(|c| c.get())
}

pub(crate) struct InspectCustomInspectGuard(bool);

impl InspectCustomInspectGuard {
    pub(crate) fn new(enabled: bool) -> Self {
        let prev = INSPECT_CUSTOM_INSPECT.with(|c| c.replace(enabled));
        Self(prev)
    }
}

impl Drop for InspectCustomInspectGuard {
    fn drop(&mut self) {
        INSPECT_CUSTOM_INSPECT.with(|c| c.set(self.0));
    }
}

thread_local! {
    static INSPECT_GETTERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INSPECT_SORTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    static INSPECT_COMPACT: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    static DEEP_EQUAL_SKIP_PROTOTYPE_FORMAT: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

fn inspect_getters_enabled() -> bool {
    INSPECT_GETTERS.with(|c| c.get())
}

fn inspect_sorted_enabled() -> bool {
    INSPECT_SORTED.with(|c| c.get())
}

fn inspect_compact_enabled() -> bool {
    INSPECT_COMPACT.with(|c| c.get())
}

fn deep_equal_skip_prototype_format_enabled() -> bool {
    DEEP_EQUAL_SKIP_PROTOTYPE_FORMAT.with(|c| c.get())
}

struct DeepEqualSkipPrototypeFormatGuard(bool);

impl DeepEqualSkipPrototypeFormatGuard {
    fn new(enabled: bool) -> Self {
        let prev = DEEP_EQUAL_SKIP_PROTOTYPE_FORMAT.with(|c| c.replace(enabled));
        Self(prev)
    }
}

impl Drop for DeepEqualSkipPrototypeFormatGuard {
    fn drop(&mut self) {
        DEEP_EQUAL_SKIP_PROTOTYPE_FORMAT.with(|c| c.set(self.0));
    }
}

pub(crate) struct InspectGettersGuard(bool);

impl InspectGettersGuard {
    pub(crate) fn new(enabled: bool) -> Self {
        let prev = INSPECT_GETTERS.with(|c| c.replace(enabled));
        Self(prev)
    }
}

impl Drop for InspectGettersGuard {
    fn drop(&mut self) {
        INSPECT_GETTERS.with(|c| c.set(self.0));
    }
}

pub(crate) struct InspectSortedGuard(bool);

impl InspectSortedGuard {
    pub(crate) fn new(enabled: bool) -> Self {
        let prev = INSPECT_SORTED.with(|c| c.replace(enabled));
        Self(prev)
    }
}

impl Drop for InspectSortedGuard {
    fn drop(&mut self) {
        INSPECT_SORTED.with(|c| c.set(self.0));
    }
}

pub(crate) struct InspectCompactGuard(bool);

impl InspectCompactGuard {
    pub(crate) fn new(enabled: bool) -> Self {
        let prev = INSPECT_COMPACT.with(|c| c.replace(enabled));
        Self(prev)
    }
}

impl Drop for InspectCompactGuard {
    fn drop(&mut self) {
        INSPECT_COMPACT.with(|c| c.set(self.0));
    }
}

unsafe fn string_header_to_string(ptr: *mut StringHeader, fallback: &str) -> String {
    if ptr.is_null() {
        return fallback.to_string();
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).unwrap_or(fallback).to_string()
}

unsafe fn format_error_headline(error_ptr: *const crate::error::ErrorHeader) -> String {
    let name_str = string_header_to_string((*error_ptr).name, "Error");
    let message_str = string_header_to_string((*error_ptr).message, "");
    if message_str.is_empty() {
        name_str
    } else {
        format!("{}: {}", name_str, message_str)
    }
}

unsafe fn format_error_stack_frame(error_ptr: *const crate::error::ErrorHeader) -> Option<String> {
    let stack = string_header_to_string((*error_ptr).stack, "");
    stack
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())
        .map(str::to_string)
}

unsafe fn format_error_array(arr_ptr: *const crate::array::ArrayHeader, depth: usize) -> String {
    if arr_ptr.is_null() {
        return "[]".to_string();
    }
    let length = (*arr_ptr).length as usize;
    if length == 0 {
        return "[]".to_string();
    }
    let data_ptr =
        (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>()) as *const f64;
    let mut out = String::from("[");
    for i in 0..length {
        out.push('\n');
        out.push_str("    ");
        out.push_str(&format_jsvalue_for_json(*data_ptr.add(i), depth + 1));
    }
    out.push('\n');
    out.push_str("  ]");
    out
}

unsafe fn format_error_value(error_ptr: *const crate::error::ErrorHeader, depth: usize) -> String {
    let headline = format_error_headline(error_ptr);
    let mut entries: Vec<(String, String)> =
        crate::node_submodules::error_user_props(error_ptr as usize)
            .into_iter()
            .filter(|(key, _)| key != "cause" && key != "errors")
            .map(|(key, value)| (key, format_jsvalue_for_json(value, depth + 1)))
            .collect();

    let cause = (*error_ptr).cause;
    if !crate::value::JSValue::from_bits(cause.to_bits()).is_undefined() {
        entries.push((
            "[cause]".to_string(),
            format_jsvalue_for_json(cause, depth + 1),
        ));
    }

    if !(*error_ptr).errors.is_null() {
        entries.push((
            "[errors]".to_string(),
            format_error_array((*error_ptr).errors, depth + 1),
        ));
    }

    if entries.is_empty() {
        return headline;
    }

    let mut out = headline;
    if let Some(frame) = format_error_stack_frame(error_ptr) {
        out.push('\n');
        out.push_str(&frame);
        out.push_str(" {");
    } else {
        out.push_str("\n{");
    }

    let last = entries.len().saturating_sub(1);
    for (idx, (label, value)) in entries.into_iter().enumerate() {
        out.push('\n');
        out.push_str("  ");
        out.push_str(&label);
        out.push_str(": ");
        out.push_str(&value);
        if idx != last {
            out.push(',');
        }
    }
    out.push('\n');
    out.push('}');
    out
}

/// #2089: a Date's `util.inspect` rendering — ISO string (unquoted) or "Invalid Date". DateCell pointer only (gated by callers).
unsafe fn date_inspect_string(value: f64) -> String {
    let s_ptr = crate::date::js_date_to_iso_string(value);
    if s_ptr.is_null() {
        return "Invalid Date".to_string();
    }
    let len = (*s_ptr).byte_len as usize;
    let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes)
        .unwrap_or("Invalid Date")
        .to_string()
}

/// `util.inspect` arm for a Temporal cell: `Temporal.X <iso>` (or
/// `[object Object]` if the cell can't be read). Returns `None` when `addr` is
/// not a Temporal cell, so the caller's `else if let Some(..)` chain falls
/// through. Cfg-paired: with the Temporal engine gated off no cell can exist, so
/// the off twin is a constant `None` (and doesn't reference the gated module).
#[cfg(feature = "temporal")]
fn temporal_inspect_arm(addr: usize, value: f64) -> Option<String> {
    if crate::temporal::is_temporal_cell_addr(addr) {
        Some(
            crate::temporal::temporal_inspect_string(value)
                .unwrap_or_else(|| "[object Object]".to_string()),
        )
    } else {
        None
    }
}

#[cfg(not(feature = "temporal"))]
fn temporal_inspect_arm(_addr: usize, _value: f64) -> Option<String> {
    None
}

/// Print multiple values from an array (console.log with spread support)
/// Takes a pointer to an ArrayHeader containing f64 values
/// Helper function to format a JSValue as a string (for spread arrays)
pub(crate) fn format_jsvalue(value: f64, depth: usize) -> String {
    // Top-level entry: clear circular-tracking state so each print starts
    // fresh and ref IDs restart at 1. See #1204.
    if depth == 0 {
        INSPECT_CIRCULAR.with(|c| c.borrow_mut().reset());
    }
    // Prevent stack overflow with deeply nested structures
    if depth > 10 {
        return "[...]".to_string();
    }

    let jsval = JSValue::from_bits(value.to_bits());

    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if jsval.is_any_string() {
            jsvalue_string_content(value).unwrap_or_else(|| "null".to_string())
        } else if jsval.is_bigint() {
            // Format BigInt by converting to string
            let ptr = jsval.as_bigint_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                if str_ptr.is_null() {
                    "0n".to_string()
                } else {
                    let len = (*str_ptr).byte_len as usize;
                    let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    let num_str = std::str::from_utf8(bytes).unwrap_or("0");
                    format!("{}n", num_str)
                }
            }
        } else if jsval.is_pointer() {
            let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
            if ptr.is_null() {
                "null".to_string()
            } else if crate::symbol::is_registered_symbol(ptr as usize) {
                // Symbols print as "Symbol(description)" inside util.inspect.
                let s = crate::symbol::js_symbol_to_string(value);
                let s_ptr = s as *const StringHeader;
                if s_ptr.is_null() {
                    "Symbol()".to_string()
                } else {
                    let len = (*s_ptr).byte_len as usize;
                    let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    std::str::from_utf8(bytes).unwrap_or("Symbol()").to_string()
                }
            } else if crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some() {
                // Typed array — Int32Array(N) [ a, b, c ] etc.
                let ta = ptr as *const crate::typedarray::TypedArrayHeader;
                crate::typedarray::format_typed_array(ta)
            } else if crate::buffer::is_data_view(ptr as usize) {
                let buf_ptr = ptr as *const crate::buffer::BufferHeader;
                array_buffer::format_data_view_value(buf_ptr)
            } else if crate::buffer::is_any_array_buffer(ptr as usize)
                && !crate::buffer::is_uint8array_buffer(ptr as usize)
            {
                let buf_ptr = ptr as *const crate::buffer::BufferHeader;
                let label = if crate::buffer::is_shared_array_buffer(ptr as usize) {
                    "SharedArrayBuffer"
                } else {
                    "ArrayBuffer"
                };
                array_buffer::format_array_buffer_value(buf_ptr, label)
            } else if crate::buffer::is_registered_buffer(ptr as usize) {
                // Buffer/Uint8Array — `<Buffer xx xx ...>`. No GC header, so
                // this must precede the GC_HEADER_SIZE arithmetic below (which
                // would read garbage one word before the BufferHeader).
                let buf_ptr = ptr as *const crate::buffer::BufferHeader;
                format_buffer_value(buf_ptr)
            } else if crate::regex::is_registered_regex(ptr as usize) {
                // RegExp literals are GC_TYPE_OBJECT with no enumerable keys
                // (generic formatter prints `{}`); render `/source/flags`
                // instead (registry-gated, before the GC-header read; #800).
                collections::format_regexp(ptr as *const crate::regex::RegExpHeader)
            } else if crate::proxy::js_proxy_is_proxy(value) != 0 {
                format_proxy_value(value, depth, false)
            } else if crate::date::is_date_cell_addr(ptr as usize) {
                // #2089: a Date is a NaN-boxed `DateCell` pointer. Node's
                // `util.inspect` prints the ISO string unquoted (or
                // `Invalid Date`). Handle before the GC-header object dispatch
                // below, which would deref the 8-byte cell as an ObjectHeader.
                date_inspect_string(value)
            } else if let Some(s) = temporal_inspect_arm(ptr as usize, value) {
                // Temporal (#4686): `util.inspect` prints `Temporal.Duration
                // <P1Y…>`. Handle before the GC-header object dispatch (the cell
                // is smaller than an ObjectHeader).
                s
            } else if crate::value::addr_class::is_handle_band(ptr as usize) {
                // Refs #421: Web Fetch (and other) handles are NaN-boxed
                // POINTER_TAG values whose payload is a small registry id, NOT
                // a heap pointer — reading the GC header at `ptr - 8` would
                // SIGSEGV. Placeholder distinguishes it from "{}".
                "{}".to_string()
            } else {
                // Use GC header to determine the actual type of the object.
                // The GC header is located GC_HEADER_SIZE bytes before the user pointer.
                let gc_header =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let mut gc_type = (*gc_header).obj_type;
                // #1448: a lazy-tape array (PERRY_JSON_TAPE) has obj_type
                // GC_TYPE_LAZY_ARRAY, which misses the array branch below and
                // prints `[object Object]`. Materialize it to a real array so
                // it inspects like one (mirrors the stringify redirect).
                let ptr: *const crate::array::ArrayHeader =
                    if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                        let m = crate::json_tape::force_materialize_lazy(
                            ptr as *mut crate::json_tape::LazyArrayHeader,
                        );
                        if m.is_null() {
                            ptr
                        } else {
                            gc_type = (*((m as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                                as *const crate::gc::GcHeader))
                                .obj_type;
                            m as *const crate::array::ArrayHeader
                        }
                    } else {
                        ptr
                    };

                if gc_type == crate::gc::GC_TYPE_ERROR {
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    format_error_value(error_ptr, depth)
                } else if gc_type == crate::gc::GC_TYPE_ARRAY {
                    // Array — format as [ elem1, elem2, ... ] matching Node.js util.inspect.
                    // Cycle check FIRST so back-edges win over depth truncation
                    // (#1204): `a=[]; a.push(a); console.log(a)` should print
                    // `<ref *1> [ [Circular *1] ]` even when depth would have
                    // collapsed nested arrays. Then the Node default depth cap
                    // (overridable via INSPECT_DEPTH_LIMIT for `%o` /
                    // `console.dir(v, { depth })`): past that, nested arrays
                    // collapse to `[Array]`.
                    if let Err(id) = inspect_enter_circular(ptr as usize) {
                        return format!("[Circular *{}]", id);
                    }
                    if depth > inspect_depth_limit() {
                        // We just pushed; finish to keep the stack balanced.
                        return inspect_finish_circular(ptr as usize, "[Array]".to_string());
                    }
                    let maybe_arr = ptr;
                    let length = (*maybe_arr).length as usize;
                    if length == 0 {
                        return inspect_finish_circular(ptr as usize, "[]".to_string());
                    }
                    let data_ptr = (maybe_arr as *const u8)
                        .add(std::mem::size_of::<crate::array::ArrayHeader>())
                        as *const f64;
                    let mut parts: Vec<String> = Vec::with_capacity(length);
                    let mut all_numeric = true;
                    for i in 0..length {
                        let elem_value = *data_ptr.add(i);
                        let elem_jsval = JSValue::from_bits(elem_value.to_bits());
                        // Quote string elements like Node's util.inspect: 'hello'
                        if elem_jsval.is_any_string() {
                            all_numeric = false;
                            let s = format_jsvalue(elem_value, depth + 1);
                            parts.push(format!("'{}'", s));
                        } else {
                            if !elem_jsval.is_number() && !elem_jsval.is_int32() {
                                all_numeric = false;
                            }
                            parts.push(format_jsvalue(elem_value, depth + 1));
                        }
                    }
                    let inner = parts.join(", ");
                    // Node uses multi-line when length > 6 or single-line exceeds breakLength (76)
                    let use_multiline =
                        !inspect_compact_enabled() || length > 6 || inner.len() + 4 > 76;
                    let body_str = if !use_multiline {
                        format!("[ {} ]", inner)
                    } else if all_numeric {
                        // Node.js groupArrayElements for numeric arrays:
                        // right-align each number to max width, compute per-line
                        // column count via Node's sqrt heuristic.
                        let max_len = parts.iter().map(|s| s.len()).max().unwrap_or(1);
                        // biasedMax = max(maxLength - 2, 1)
                        let biased_max = max_len.saturating_sub(2).max(1);
                        // cols_by_sqrt = round(sqrt(2.5 * biasedMax * N) / biasedMax)
                        let cols_by_sqrt = ((2.5_f64 * biased_max as f64 * length as f64).sqrt()
                            / biased_max as f64)
                            .round() as usize;
                        // cols_by_width = ceil(breakLength / (maxLen + 2)); breakLength=76
                        let actual_max = max_len + 2;
                        let cols_by_width = 76_usize.div_ceil(actual_max);
                        let columns = cols_by_sqrt
                            .min(cols_by_width.max(1))
                            .min(12) // compact(3) * 4
                            .min(15) // absolute max per Node
                            .max(1);
                        let indent = "  ";
                        let mut lines: Vec<String> = parts
                            .chunks(columns)
                            .map(|chunk| {
                                let elems: Vec<String> = chunk
                                    .iter()
                                    .map(|s| format!("{:>width$}", s, width = max_len))
                                    .collect();
                                format!("{}{}", indent, elems.join(", "))
                            })
                            .collect();
                        // Trailing comma on every line but the last (Node format)
                        let n_lines = lines.len();
                        for line in lines.iter_mut().take(n_lines - 1) {
                            line.push(',');
                        }
                        format!("[\n{}\n]", lines.join("\n"))
                    } else {
                        // Non-numeric multi-line: short arrays of wide string
                        // entries print one item per row in Node's compact
                        // inspect layout.
                        let indent = "  ";
                        let chunk_size = if length <= 6 { 1 } else { 4 };
                        let mut row_strs: Vec<String> = parts
                            .chunks(chunk_size)
                            .map(|chunk| format!("{}{}", indent, chunk.join(", ")))
                            .collect();
                        let n = row_strs.len();
                        for line in row_strs.iter_mut().take(n - 1) {
                            line.push(',');
                        }
                        format!("[\n{}\n]", row_strs.join("\n"))
                    };
                    inspect_finish_circular(ptr as usize, body_str)
                } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                    // Object — check for keys_array. Cycle check FIRST so the
                    // self-referencing case wins over the depth-2 collapse to
                    // `[Object]` (#1204). The depth cap is overridable via
                    // INSPECT_DEPTH_LIMIT for `%o` / `console.dir(v, { depth })`.
                    let obj_ptr = ptr as *const crate::object::ObjectHeader;
                    if let Some(body) = format_weak_wrapper(obj_ptr, depth) {
                        return body;
                    }
                    if let Err(id) = inspect_enter_circular(ptr as usize) {
                        return format!("[Circular *{}]", id);
                    }
                    if depth > inspect_depth_limit() {
                        return inspect_finish_circular(ptr as usize, "[Object]".to_string());
                    }
                    let _keys_array = (*obj_ptr).keys_array;

                    // Always route through `format_object_as_json` so the
                    // `[util.inspect.custom]` hook lookup runs even for
                    // objects with no string-keyed fields (#1247 / #1252):
                    // an object whose only own key is the inspect symbol
                    // has `keys_array == null` and the prior fast-path
                    // skipped the hook entirely, printing `{}`. The
                    // formatter itself short-circuits to `{}` when no
                    // hook fires and the keys_array is empty.
                    let body_str = format_object_as_json(obj_ptr, depth);
                    inspect_finish_circular(ptr as usize, body_str)
                } else if gc_type == crate::gc::GC_TYPE_MAP {
                    collections::format_map_with_cycle(ptr, depth)
                } else if gc_type == crate::gc::GC_TYPE_SET {
                    collections::format_set_with_cycle(ptr, depth)
                } else if gc_type == crate::gc::GC_TYPE_CLOSURE {
                    format_function_for_console(ptr as *const crate::closure::ClosureHeader)
                } else if gc_type == crate::gc::GC_TYPE_PROMISE {
                    "Promise { <pending> }".to_string()
                } else {
                    // Safe fallback for unknown GC types — avoid heuristic
                    // pointer interpretation which can crash on closures,
                    // sets, maps, etc.
                    "[object Object]".to_string()
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
            // Regular number — but first check for raw (non-NaN-boxed) heap
            // pointers. The codegen sometimes returns a raw
            // i64 buffer pointer bitcast directly to f64 (no POINTER_TAG), so
            // `jsval.is_pointer()` is false yet the bit pattern is a valid
            // buffer address. Detect this case by looking up the raw bits
            // in the thread-local BUFFER_REGISTRY.
            let raw_bits = value.to_bits();
            if raw_bits > 0x1000 && (raw_bits >> 48) == 0 {
                if crate::typedarray::lookup_typed_array_kind(raw_bits as usize).is_some() {
                    let ta = raw_bits as *const crate::typedarray::TypedArrayHeader;
                    return crate::typedarray::format_typed_array(ta);
                }
                if crate::buffer::is_registered_buffer(raw_bits as usize) {
                    let buf_ptr = raw_bits as *const crate::buffer::BufferHeader;
                    return format_buffer_value(buf_ptr);
                }
            }
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT {
                (n as i64).to_string()
            } else {
                format_finite_number_js(n)
            }
        }
    }
}

/// Format a Node.js Buffer as `<Buffer xx yy zz ...>` (lowercase hex bytes
/// separated by single spaces). Mirrors Node's `util.inspect` output for
/// Buffer / Uint8Array. Node truncates after 50 bytes with `... N more bytes`
/// but we emit the whole buffer for now (tests use small buffers).
unsafe fn format_buffer_value(buf_ptr: *const crate::buffer::BufferHeader) -> String {
    if buf_ptr.is_null() {
        return "<Buffer >".to_string();
    }
    let len = (*buf_ptr).length as usize;
    let data = (buf_ptr as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>());
    let bytes = std::slice::from_raw_parts(data, len);

    // If this buffer was created via `new Uint8Array(...)`, format it Node-style
    // as `Uint8Array(N) [ a, b, c ]` rather than `<Buffer aa bb cc>`.
    if crate::buffer::is_uint8array_buffer(buf_ptr as usize) {
        if len == 0 {
            return "Uint8Array(0) []".to_string();
        }
        let mut out = format!("Uint8Array({}) [", len);
        for (i, b) in bytes.iter().enumerate() {
            if i == 0 {
                out.push(' ');
            } else {
                out.push_str(", ");
            }
            out.push_str(&format!("{}", *b));
        }
        out.push_str(" ]");
        return out;
    }

    // Node caps at 50 bytes then shows "... N more bytes"
    let display_len = len.min(50);
    let mut out = String::with_capacity(9 + display_len * 3);
    out.push_str("<Buffer");
    for b in &bytes[..display_len] {
        out.push(' ');
        out.push_str(&format!("{:02x}", b));
    }
    if len > display_len {
        out.push_str(&format!(" ... {} more bytes", len - display_len));
    }
    out.push('>');
    out
}

fn format_proxy_value(value: f64, depth: usize, json: bool) -> String {
    let target = crate::proxy::js_proxy_target(value);
    if !inspect_show_proxy() {
        let target_str = if json {
            format_jsvalue_for_json(target, depth)
        } else {
            format_jsvalue(target, depth)
        };
        return format!("Proxy({target_str})");
    }

    let handler = crate::proxy::js_proxy_handler(value);
    let target_str = if json {
        format_jsvalue_for_json(target, depth + 1)
    } else {
        format_jsvalue(target, depth + 1)
    };
    let handler_str = if json {
        format_jsvalue_for_json(handler, depth + 1)
    } else {
        format_jsvalue(handler, depth + 1)
    };
    format!("Proxy [ {}, {} ]", target_str, handler_str)
}

fn format_weak_wrapper(
    obj_ptr: *const crate::object::ObjectHeader,
    depth: usize,
) -> Option<String> {
    use crate::weakref::WeakWrapperKind;

    match crate::weakref::weak_wrapper_kind(obj_ptr)? {
        WeakWrapperKind::WeakRef | WeakWrapperKind::FinalizationRegistry => {
            crate::weakref::weak_wrapper_inspect_label(obj_ptr).map(str::to_string)
        }
        WeakWrapperKind::WeakMap | WeakWrapperKind::WeakSet if !inspect_show_hidden() => {
            crate::weakref::weak_wrapper_inspect_label(obj_ptr).map(str::to_string)
        }
        WeakWrapperKind::WeakMap => {
            let parts = crate::weakref::weak_collection_entries(obj_ptr)
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{} => {}",
                        format_jsvalue(key, depth + 1),
                        format_jsvalue_for_json(value, depth + 1)
                    )
                })
                .collect::<Vec<_>>();
            Some(format!("WeakMap {{ {} }}", parts.join(", ")))
        }
        WeakWrapperKind::WeakSet => {
            let parts = crate::weakref::weak_collection_entries(obj_ptr)
                .into_iter()
                .map(|(key, _)| format_jsvalue(key, depth + 1))
                .collect::<Vec<_>>();
            Some(format!("WeakSet {{ {} }}", parts.join(", ")))
        }
    }
}

/// Format an object as JSON-like string
/// Reads keys from the keys_array and values from the fields.
///
/// `depth` is the current nesting level: `format_jsvalue`/`format_jsvalue_for_json`
/// invoke this with `depth = 0` for the outermost object, and each nested
/// object recurses with `depth + 1`. The hard cap at depth > 10 remains as a
/// crash safety net for cyclic structures; the Node-style `[Object]` truncation
/// at depth > 2 is enforced by `format_jsvalue_for_json` on the way in.
unsafe fn format_object_as_json(
    obj_ptr: *const crate::object::ObjectHeader,
    depth: usize,
) -> String {
    if depth > 10 {
        return "{...}".to_string();
    }

    let obj_addr = obj_ptr as usize;

    // `[util.inspect.custom]` hook: when the object carries a symbol-keyed
    // entry for `Symbol.for("nodejs.util.inspect.custom")` and the
    // `customInspect` inspect option is enabled (Node default for
    // `util.inspect` and `console.log` — `console.dir` opts out via
    // `InspectCustomInspectGuard`), invoke it and use the return value
    // verbatim when it's a string, or recursively inspect otherwise. The
    // hook itself runs with `customInspect` temporarily disabled to prevent
    // unbounded recursion if the hook returns `this`. Refs #1201.
    if inspect_custom_inspect_enabled() {
        let custom_sym = crate::symbol::inspect_custom_symbol_ptr();
        if custom_sym != 0 {
            let entries = crate::symbol::clone_symbol_entries_for_obj_ptr(obj_addr);
            for (sym_ptr, val_bits) in &entries {
                if *sym_ptr != custom_sym {
                    continue;
                }
                let val_tag = val_bits & 0xFFFF_0000_0000_0000;
                if val_tag != 0x7FFD_0000_0000_0000 {
                    break;
                }
                let val_ptr = (val_bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
                if val_ptr.is_null() || (val_ptr as usize) < 0x10000 {
                    break;
                }
                let gc_header =
                    val_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                if (*gc_header).obj_type != crate::gc::GC_TYPE_CLOSURE {
                    break;
                }
                let closure_ptr = val_ptr as *const crate::closure::ClosureHeader;
                let _guard = InspectCustomInspectGuard::new(false);
                // Node invokes the hook as `hook.call(this, depth, options, inspect)`
                // — see #1247. `depth` is the REMAINING recursion budget (counts
                // down from `util.inspect`'s `depth` option, default 2). Perry's
                // internal `depth` counts UP from 0 so we invert it. `options`
                // is a placeholder object — populating its fields properly is
                // its own follow-up. `inspect` is undefined since we don't yet
                // expose a JS-callable equivalent.
                let remaining = inspect_depth_limit().saturating_sub(depth) as f64;
                let options_obj = crate::object::js_object_alloc(0, 0);
                let options_arg = crate::value::js_nanbox_pointer(options_obj as i64);
                let undef_arg = f64::from_bits(crate::value::TAG_UNDEFINED);
                let ret = crate::closure::js_closure_call3(
                    closure_ptr,
                    remaining,
                    options_arg,
                    undef_arg,
                );
                let ret_jv = crate::value::JSValue::from_bits(ret.to_bits());
                if ret_jv.is_any_string() {
                    return jsvalue_string_content(ret).unwrap_or_default();
                }
                // #1251: non-string return values count as one nesting
                // level — but the hook itself was reached from the
                // formatter at `depth`, so the return value's nested
                // structure starts at `depth` (NOT `depth + 1`). Node
                // ends up truncating `[Object]` at the same boundary
                // because both formatters increment by 1 per descent.
                return format_jsvalue(ret, depth);
            }
        }

        // #1248: class-method `[util.inspect.custom]() {}` is not stored in
        // the per-instance symbol side table — HIR renames it to
        // `__perry_inspect_custom__` and registers it on the class vtable
        // (see crates/perry-hir/src/lower_decl/class_decl.rs). Walk the
        // object's class chain when the instance lookup misses.
        let class_id = (*obj_ptr).class_id;
        if class_id != 0 {
            if let Some((func_ptr, param_count, has_synthetic_arguments, has_rest)) =
                crate::object::lookup_class_method_in_chain(class_id, "__perry_inspect_custom__")
            {
                let _guard = InspectCustomInspectGuard::new(false);
                let remaining = inspect_depth_limit().saturating_sub(depth) as f64;
                let options_obj = crate::object::js_object_alloc(0, 0);
                let options_arg = crate::value::js_nanbox_pointer(options_obj as i64);
                let undef_arg = f64::from_bits(crate::value::TAG_UNDEFINED);
                let args = [remaining, options_arg, undef_arg];
                let ret = crate::object::call_vtable_method(
                    func_ptr,
                    obj_ptr as i64,
                    args.as_ptr(),
                    args.len(),
                    param_count,
                    has_synthetic_arguments,
                    has_rest,
                );
                let ret_jv = crate::value::JSValue::from_bits(ret.to_bits());
                if ret_jv.is_any_string() {
                    return jsvalue_string_content(ret).unwrap_or_default();
                }
                return format_jsvalue(ret, depth);
            }
        }
    }

    let boxed_base = boxed_primitives::boxed_primitive_base_for_object(obj_ptr);
    // A boxed `String` exposes its characters as integer-index own properties
    // (`"0".."len-1"`). Node folds those into the `[String: '…']` base and never
    // lists them in the `{ … }` body — only extra own keys appear. Skip them.
    let boxed_string_char_count = boxed_primitives::boxed_string_char_index_count(obj_ptr);
    let class_name = {
        let class_id = (*obj_ptr).class_id;
        if class_id == 0 {
            None
        } else {
            crate::object::class_name_for_id(class_id).filter(|name| !name.is_empty())
        }
    };
    let has_class_name = class_name.is_some();
    let class_name_ref = if deep_equal_skip_prototype_format_enabled() {
        None
    } else {
        class_name.as_deref()
    };
    // Node prefixes a null-prototype object with `[Object: null prototype]`
    // (e.g. `Object.create(null)`), since it has no constructor to name.
    // The flag is set at allocation by `js_object_alloc_null_proto`.
    let is_null_proto = {
        let gc =
            (obj_ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        (*gc)._reserved & crate::gc::OBJ_FLAG_NULL_PROTO != 0
    };
    // The display prefix before the `{ … }` body: a real class/constructor
    // name when present, otherwise `[Object: null prototype]` for a
    // null-proto plain object, otherwise nothing. (Distinct from
    // `class_name_ref`/`has_class_name`, which drive the private-field skip
    // and must reflect only a genuine class.)
    let name_prefix: Option<String> = match class_name_ref {
        Some(name) => Some(name.to_string()),
        None if boxed_base.is_none() && is_null_proto => {
            Some("[Object: null prototype]".to_string())
        }
        None => None,
    };
    let empty_object = || {
        if let Some(base) = boxed_base.as_deref() {
            return base.to_string();
        }
        match name_prefix.as_deref() {
            Some(name) => format!("{name} {{}}"),
            None => "{}".to_string(),
        }
    };

    let keys_array = (*obj_ptr).keys_array;
    let key_count = if keys_array.is_null() {
        0
    } else {
        crate::array::js_array_length(keys_array) as usize
    };

    // Honor `Object.defineProperty(..., { enumerable: false })`. By default
    // we include every key in the `keys_array` (enumerability is rarely
    // overridden, so the descriptor table is empty — early-out via the
    // global flag avoids per-key lookups on the common path). When at
    // least one descriptor exists, consult it per key:
    //   - enumerable + any case → print as `key: value`
    //   - non-enumerable + showHidden → print as `[key]: value` (Node-style)
    //   - non-enumerable + !showHidden → skip
    // See #1200.
    let show_hidden = inspect_show_hidden();
    let descriptors_in_use = crate::object::descriptors_in_use();

    let mut string_parts: Vec<(String, String)> = Vec::with_capacity(key_count);

    for i in 0..key_count {
        // Get the key (NaN-boxed string pointer)
        let key_val = crate::array::js_array_get(keys_array, i as u32);
        let key_str = if key_val.is_string() {
            let key_ptr = key_val.as_string_ptr();
            if key_ptr.is_null() {
                continue;
            }
            let len = (*key_ptr).byte_len as usize;
            let data = (key_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        } else {
            continue;
        };

        // Perry stores private class fields in the regular key table, but
        // Node's util.inspect never exposes them, even with showHidden.
        if has_class_name && key_str.starts_with('#') {
            continue;
        }

        // Hide a boxed String's character index properties (`"0".."len-1"`):
        // they are rendered by the `[String: '…']` base, not the body.
        if let Some(char_count) = boxed_string_char_count {
            if let Ok(idx) = key_str.parse::<usize>() {
                if idx < char_count {
                    continue;
                }
            }
        }

        // `String` exotic objects always have a non-enumerable own `length`.
        // Its descriptor is installed through the gate-neutral built-in
        // descriptor path, so the process-wide user-descriptor fast-path flag
        // can legitimately still be false. Do not let that optimization make
        // util.inspect print `length` as an ordinary enumerable property.
        let is_boxed_string_length = boxed_string_char_count.is_some() && key_str == "length";
        let is_enumerable = if is_boxed_string_length {
            false
        } else if descriptors_in_use {
            crate::object::get_property_attrs(obj_addr, &key_str)
                .map(|a| a.enumerable())
                .unwrap_or(true)
        } else {
            true
        };
        if !is_enumerable && !show_hidden {
            continue;
        }

        let value_str =
            if let Some(acc) = crate::object::get_accessor_descriptor(obj_addr, &key_str) {
                format_accessor_property(acc, depth)
            } else {
                let value = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
                format_jsvalue_for_json(value, depth + 1)
            };

        let rendered = if is_enumerable {
            format!("{}: {}", key_str, value_str)
        } else {
            // Node wraps non-enumerable keys in brackets under showHidden.
            format!("[{}]: {}", key_str, value_str)
        };
        string_parts.push((key_str, rendered));
    }

    if inspect_sorted_enabled() {
        string_parts.sort_by(|(left, _), (right, _)| left.cmp(right));
    }

    let mut parts: Vec<String> = string_parts
        .into_iter()
        .map(|(_, rendered)| rendered)
        .collect();

    // Append symbol-keyed properties last (matches Node's enumeration order:
    // string keys first, then symbol keys). Perry's symbol side table only
    // stores enumerable own symbol props today, and Node renders those labels
    // without brackets: `Symbol(<desc>): <value>`. Brackets are reserved for
    // non-enumerable symbol props when `showHidden` is enabled. Refs #1201.
    let sym_entries = crate::symbol::clone_symbol_entries_for_obj_ptr(obj_addr);
    for (sym_ptr_usize, val_bits) in sym_entries {
        let sym_f64 = f64::from_bits(
            0x7FFD_0000_0000_0000u64 | (sym_ptr_usize as u64 & 0x0000_FFFF_FFFF_FFFFu64),
        );
        let sym_label = {
            let s_ptr = crate::symbol::js_symbol_to_string(sym_f64) as *const StringHeader;
            if s_ptr.is_null() {
                "Symbol()".to_string()
            } else {
                let len = (*s_ptr).byte_len as usize;
                let data = (s_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                let bytes = std::slice::from_raw_parts(data, len);
                std::str::from_utf8(bytes).unwrap_or("Symbol()").to_string()
            }
        };
        let value = f64::from_bits(val_bits);
        let value_str = format_jsvalue_for_json(value, depth + 1);
        parts.push(format!("{}: {}", sym_label, value_str));
    }

    if parts.is_empty() {
        return empty_object();
    }
    let single_line = match (boxed_base.as_deref(), name_prefix.as_deref()) {
        (Some(base), _) => format!("{} {{ {} }}", base, parts.join(", ")),
        (None, Some(name)) => format!("{} {{ {} }}", name, parts.join(", ")),
        (None, None) => format!("{{ {} }}", parts.join(", ")),
    };
    // Node's `util.inspect` switches to multi-line layout when the single-line
    // rendering would exceed `breakLength` (default 80). The threshold is
    // measured against the body alone — we approximate with 72 here because
    // outer callers (arrays, nested objects) may prepend indentation that
    // pushes the final width past 80. Empty / short bodies stay on one line
    // so `console.dir({ foo: 1 })` keeps printing `{ foo: 1 }`. Refs #1201.
    //
    // #1249: if any rendered child already contains a newline (its own
    // nested formatter chose multi-line), the outer MUST also break — keeping
    // it single-line would re-emit the child's continuation lines without our
    // indent prefix, producing a left-aligned inner body inside an indented
    // outer body.
    let any_child_multiline = parts.iter().any(|p| p.contains('\n'));
    if inspect_compact_enabled() && !any_child_multiline && single_line.len() <= 72 {
        return single_line;
    }
    let indent = "  ";
    let body = parts
        .iter()
        .map(|p| format!("{}{}", indent, p.replace('\n', "\n  ")))
        .collect::<Vec<_>>()
        .join(",\n");
    match (boxed_base.as_deref(), name_prefix.as_deref()) {
        (Some(base), _) => format!("{} {{\n{}\n}}", base, body),
        (None, Some(name)) => format!("{} {{\n{}\n}}", name, body),
        (None, None) => format!("{{\n{}\n}}", body),
    }
}

fn format_accessor_property(acc: crate::object::AccessorDescriptor, depth: usize) -> String {
    let has_getter = acc.get != 0;
    let has_setter = acc.set != 0;
    let label = match (has_getter, has_setter) {
        (true, true) => "Getter/Setter",
        (true, false) => "Getter",
        (false, true) => "Setter",
        (false, false) => return "undefined".to_string(),
    };

    if inspect_getters_enabled() && has_getter {
        let closure =
            (acc.get & crate::value::POINTER_MASK) as *const crate::closure::ClosureHeader;
        if !closure.is_null() {
            let value = unsafe { crate::closure::js_closure_call0(closure) };
            return format!("[{}: {}]", label, format_jsvalue_for_json(value, depth + 1));
        }
    }

    format!("[{}]", label)
}

/// Format a JSValue for JSON output (strings get quotes)
///
/// Node's `util.inspect` default options truncate nested objects at depth 2 —
/// anything past that prints as `[Object]` / `[Array]`. We mirror that so
/// `console.log({ a: { b: { c: { d: 1 } } } })` matches Node byte-for-byte.
/// The hard guard at depth > 10 remains as a crash safety net for pathological
/// cyclic structures.
fn format_jsvalue_for_json(value: f64, depth: usize) -> String {
    // Top-level callers (`deep_equal`, JSON stringify) reach this directly,
    // not through `format_jsvalue`. Reset circular state at depth=0 so we
    // don't accumulate stale ref IDs across unrelated print/compare calls.
    if depth == 0 {
        INSPECT_CIRCULAR.with(|c| c.borrow_mut().reset());
    }
    if depth > 10 {
        return "\"...\"".to_string();
    }

    let jsval = JSValue::from_bits(value.to_bits());

    unsafe {
        if jsval.is_undefined() {
            "undefined".to_string()
        } else if jsval.is_null() {
            "null".to_string()
        } else if jsval.is_bool() {
            jsval.as_bool().to_string()
        } else if jsval.is_any_string() {
            // Escape and quote strings for JSON-like output. SSO + heap
            // strings handled identically via the central decoder.
            let s = jsvalue_string_content(value).unwrap_or_default();
            format!("'{}'", escape_string(&s))
        } else if jsval.is_bigint() {
            let ptr = jsval.as_bigint_ptr();
            if ptr.is_null() {
                "null".to_string()
            } else {
                let str_ptr = crate::bigint::js_bigint_to_string(ptr);
                if str_ptr.is_null() {
                    "0n".to_string()
                } else {
                    let len = (*str_ptr).byte_len as usize;
                    let data = (str_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                    let bytes = std::slice::from_raw_parts(data, len);
                    let num_str = std::str::from_utf8(bytes).unwrap_or("0");
                    format!("{}n", num_str)
                }
            }
        } else if jsval.is_pointer() {
            let ptr: *const crate::array::ArrayHeader = jsval.as_pointer();
            if ptr.is_null() {
                "null".to_string()
            } else {
                // #1457: classify the heap type from the GC header, NOT from
                // `*(ptr as *const u32)`. That first word is an ArrayHeader's
                // `length`, so a length-2 array collided with
                // `OBJECT_TYPE_ERROR == 2` and was misread as an Error —
                // dereferencing element bits as name/message string pointers
                // and segfaulting (any object with a 2-element array field,
                // tape or not, crashed `console.log`). Mirrors the main
                // `format_jsvalue` formatter's GC-header dispatch.
                //
                // A small registry-id handle (`< 0x100000`, e.g. a Web Fetch
                // Request) carries no GC header, so reading `ptr - 8` would
                // deref unmapped memory — print a placeholder instead.
                if crate::proxy::js_proxy_is_proxy(value) != 0 {
                    format_proxy_value(value, depth, true)
                } else if crate::date::is_date_cell_addr(ptr as usize) {
                    // #2089: Date inside an inspected object — ISO string
                    // unquoted (or `Invalid Date`), not the 8-byte cell deref'd
                    // as an object.
                    date_inspect_string(value)
                } else if let Some(s) = temporal_inspect_arm(ptr as usize, value) {
                    // Temporal value inside an inspected object → `Temporal.X <iso>`.
                    s
                } else if crate::value::addr_class::is_handle_band(ptr as usize) {
                    "[object Object]".to_string()
                } else if crate::symbol::is_registered_symbol(ptr as usize)
                    || crate::regex::is_registered_regex(ptr as usize)
                    || crate::buffer::is_registered_buffer(ptr as usize)
                    || crate::typedarray::lookup_typed_array_kind(ptr as usize).is_some()
                {
                    // Symbol / RegExp / Buffer / TypedArray field values need
                    // type-specific rendering this JSON-ish formatter never
                    // implemented — they collapsed to `[object Object]` / `{}`
                    // (#800). Delegate to `format_jsvalue`, which gates these
                    // on the same registries BEFORE any GC-header read (Buffers
                    // carry no GC header, so the read below would be garbage).
                    format_jsvalue(value, depth)
                } else {
                    let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *const crate::gc::GcHeader;
                    let mut gc_type = (*gc_header).obj_type;
                    // #1448: materialize a nested lazy-tape array so it inspects
                    // as a real array. Reading its LazyArrayHeader as an
                    // ArrayHeader below would otherwise read garbage and SIGSEGV.
                    let ptr: *const crate::array::ArrayHeader =
                        if gc_type == crate::gc::GC_TYPE_LAZY_ARRAY {
                            let m = crate::json_tape::force_materialize_lazy(
                                ptr as *mut crate::json_tape::LazyArrayHeader,
                            );
                            if m.is_null() {
                                ptr
                            } else {
                                gc_type = (*((m as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                                    as *const crate::gc::GcHeader))
                                    .obj_type;
                                m as *const crate::array::ArrayHeader
                            }
                        } else {
                            ptr
                        };

                    if gc_type == crate::gc::GC_TYPE_ERROR {
                        let error_ptr = ptr as *const crate::error::ErrorHeader;
                        format_error_value(error_ptr, depth)
                    } else if gc_type == crate::gc::GC_TYPE_ARRAY {
                        // Cycle check FIRST so back-edges always print as
                        // `[Circular *N]` regardless of depth (#1204). The
                        // depth cap is overridable via INSPECT_DEPTH_LIMIT
                        // for `%o` / `console.dir(v, { depth })`.
                        if let Err(id) = inspect_enter_circular(ptr as usize) {
                            return format!("[Circular *{}]", id);
                        }
                        if depth > inspect_depth_limit() {
                            return inspect_finish_circular(ptr as usize, "[Array]".to_string());
                        }
                        let maybe_arr = ptr;
                        let length = (*maybe_arr).length as usize;
                        if length > 1_000_000 {
                            return inspect_finish_circular(ptr as usize, "[Array]".to_string());
                        }
                        let data_ptr = (maybe_arr as *const u8)
                            .add(std::mem::size_of::<crate::array::ArrayHeader>())
                            as *const f64;
                        let mut parts: Vec<String> = Vec::with_capacity(length);
                        for i in 0..length {
                            let elem_value = *data_ptr.add(i);
                            parts.push(format_jsvalue_for_json(elem_value, depth + 1));
                        }
                        // Node formats empty arrays as `[]` and non-empty
                        // arrays with a space inside the brackets:
                        // `[ 1, 2, 3 ]`. Match byte-for-byte.
                        let body_str = if length == 0 {
                            "[]".to_string()
                        } else {
                            format!("[ {} ]", parts.join(", "))
                        };
                        inspect_finish_circular(ptr as usize, body_str)
                    } else if gc_type == crate::gc::GC_TYPE_OBJECT {
                        // Cycle check FIRST so back-edges win over the
                        // depth-limit collapse to `[Object]` (#1204). The
                        // depth cap is overridable via INSPECT_DEPTH_LIMIT
                        // for `%o` / `console.dir(v, { depth })`.
                        let obj_ptr = ptr as *const crate::object::ObjectHeader;
                        if let Some(body) = format_weak_wrapper(obj_ptr, depth) {
                            return body;
                        }
                        if let Err(id) = inspect_enter_circular(ptr as usize) {
                            return format!("[Circular *{}]", id);
                        }
                        if depth > inspect_depth_limit() {
                            return inspect_finish_circular(ptr as usize, "[Object]".to_string());
                        }
                        let keys_array = (*obj_ptr).keys_array;
                        let body_str = if !keys_array.is_null()
                            && (keys_array as usize) > 0x10000
                            && ((keys_array as u64) >> 48) == 0
                        {
                            format_object_as_json(obj_ptr, depth)
                        } else {
                            "[object Object]".to_string()
                        };
                        inspect_finish_circular(ptr as usize, body_str)
                    } else if gc_type == crate::gc::GC_TYPE_MAP {
                        collections::format_map_with_cycle(ptr, depth)
                    } else if gc_type == crate::gc::GC_TYPE_SET {
                        collections::format_set_with_cycle(ptr, depth)
                    } else if gc_type == crate::gc::GC_TYPE_CLOSURE {
                        // Function-valued field: route through the same display
                        // path as `format_jsvalue` so the registered function
                        // name flows out instead of `[object Object]`.
                        format_function_for_console(ptr as *const crate::closure::ClosureHeader)
                    } else {
                        "[object Object]".to_string()
                    }
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
            // A TypedArray field is a RAW (non-NaN-boxed) heap pointer, so it
            // lands here, not in the pointer branch; redirect it (#800).
            if let Some(s) = collections::raw_heap_pointer_display(value, depth) {
                return s;
            }
            let n = value;
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n > 0.0 {
                    "Infinity".to_string()
                } else {
                    "-Infinity".to_string()
                }
            } else if is_negative_zero(n) {
                "-0".to_string()
            } else if n.fract() == 0.0 && n.abs() < INT_EXACT_FASTPATH_LIMIT {
                (n as i64).to_string()
            } else {
                format_finite_number_js(n)
            }
        }
    }
}

/// Escape special characters in a string for display
fn escape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => result.push_str("\\\\"),
            '\'' => result.push_str("\\'"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            _ => result.push(c),
        }
    }
    result
}

#[inline]
fn looks_like_raw_heap_pointer(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        return false;
    }
    let addr = bits as usize;
    // Compare in u64 so the 2^47 upper bound stays in range on 32-bit targets
    // (arm64_32 watchOS, wasm32), where it's a no-op — no addresses that high.
    (0x1000..0x8000_0000_0000u64).contains(&(addr as u64))
        && addr >= crate::gc::GC_HEADER_SIZE + 0x1000
}

fn formatted_deep_equal(left: f64, right: f64, skip_prototype: bool) -> bool {
    let _guard = DeepEqualSkipPrototypeFormatGuard::new(skip_prototype);
    format_jsvalue_for_json(left, 0) == format_jsvalue_for_json(right, 0)
}

fn js_util_deep_strict_equal_bool(
    left: f64,
    right: f64,
    depth: usize,
    skip_prototype: bool,
) -> bool {
    if depth > 64 {
        return formatted_deep_equal(left, right, skip_prototype);
    }
    let left_value = crate::value::JSValue::from_bits(left.to_bits());
    let right_value = crate::value::JSValue::from_bits(right.to_bits());
    let left_boxed = boxed_primitives::boxed_primitive_payload(left);
    let right_boxed = boxed_primitives::boxed_primitive_payload(right);
    if left_boxed.is_some() || right_boxed.is_some() {
        return match (left_boxed, right_boxed) {
            (Some((left_class, left_payload)), Some((right_class, right_payload)))
                if left_class == right_class =>
            {
                js_util_deep_strict_equal_bool(
                    left_payload,
                    right_payload,
                    depth + 1,
                    skip_prototype,
                )
            }
            _ => false,
        };
    }
    if let Some(equal) =
        collection_equality::deep_strict_collection_equal(left, right, depth, skip_prototype)
    {
        return equal;
    }
    if let Some(equal) = typed_array_equality::deep_strict_typed_array_equal(left, right) {
        return equal;
    }
    if identity_equality::is_identity_only_deep_equal_value(left)
        || identity_equality::is_identity_only_deep_equal_value(right)
    {
        return left.to_bits() == right.to_bits();
    }
    let has_tagged_heap_operand = left_value.is_pointer() || right_value.is_pointer();
    let has_raw_heap_operand =
        looks_like_raw_heap_pointer(left) || looks_like_raw_heap_pointer(right);
    if has_raw_heap_operand {
        false
    } else if has_tagged_heap_operand {
        // #2934: Node's default deepStrictEqual is prototype-sensitive — two
        // objects with the same own properties but different `[[Prototype]]`
        // are not equal. Gate before comparing the formatted body.
        if !skip_prototype && prototype_equality::prototypes_differ(left, right) {
            return false;
        }
        formatted_deep_equal(left, right, skip_prototype)
    } else {
        crate::value::js_jsvalue_equals(left, right) != 0
            || formatted_deep_equal(left, right, skip_prototype)
    }
}

#[no_mangle]
pub extern "C" fn js_util_is_deep_strict_equal(left: f64, right: f64) -> f64 {
    let equal = js_util_deep_strict_equal_bool(left, right, 0, false);
    f64::from_bits(crate::value::JSValue::bool(equal).bits())
}

pub fn js_util_is_deep_strict_equal_skip_prototype(left: f64, right: f64) -> f64 {
    let equal = js_util_deep_strict_equal_bool(left, right, 0, true);
    f64::from_bits(crate::value::JSValue::bool(equal).bits())
}

/// Print an array in the format [element1, element2, ...]
#[no_mangle]
pub extern "C" fn js_array_print(arr_ptr: *const crate::array::ArrayHeader) {
    if arr_ptr.is_null() {
        println!("null");
        return;
    }

    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;

        let mut parts: Vec<String> = Vec::with_capacity(length);
        for i in 0..length {
            let value = *data_ptr.add(i);
            parts.push(format_jsvalue_for_json(value, 0));
        }
        println!("[{}]", parts.join(", "));
    }
}
