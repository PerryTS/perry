//! Value formatting for `console.log`/`util.inspect`-style output.
//!
//! Split out of the original monolithic `builtins.rs` (#topic: split-large-files).
//! Contains the shared circular-reference tracking, inspect-depth/showHidden
//! guards, function-name registry, `format_jsvalue` / `format_jsvalue_for_json`
//! recursion, plus `util.format` / `util.inspect` entry points.

#[cfg(feature = "ohos-napi")]
use super::println;
use super::*;

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
/// default of 2. The `%o` format specifier raises this temporarily so
/// `console.log("%o", deep)` renders deeper than `%O` / a bare arg
/// (Node distinguishes them: `%o` is effectively unbounded, `%O` uses
/// the default cap of 2).
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
    let label = unsafe {
        let func_ptr = (*closure_ptr).func_ptr;
        if !func_ptr.is_null() {
            if let Ok(map) = function_name_registry().lock() {
                if let Some(name) = map.get(&(func_ptr as usize)) {
                    if !name.is_empty() {
                        format!("[Function: {}]", name)
                    } else {
                        "[Function (anonymous)]".to_string()
                    }
                } else {
                    "[Function (anonymous)]".to_string()
                }
            } else {
                "[Function (anonymous)]".to_string()
            }
        } else {
            "[Function (anonymous)]".to_string()
        }
    };

    // Snapshot user-attached own properties and filter out the built-in
    // function slots that Node hides from `util.inspect`. Node prints
    // these only when the user reassigned them — but `prototype` and
    // `name` are runtime-allocated on every function, so always hiding
    // them yields parity for the common case (`f.x = 1`).
    let props = crate::closure::closure_dynamic_props_snapshot(closure_ptr as usize);
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

/// Per-thread override for the `showHidden` inspect option. Defaults to
/// `false` (Node default): `util.inspect` / `console.log` only show
/// enumerable properties. `console.dir(value, { showHidden: true })`
/// flips this for the duration of the print so non-enumerable props
/// surface in `[bracketed]` form. See #1200.
thread_local! {
    static INSPECT_SHOW_HIDDEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(crate) fn inspect_show_hidden() -> bool {
    INSPECT_SHOW_HIDDEN.with(|c| c.get())
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
            } else if crate::buffer::is_registered_buffer(ptr as usize) {
                // Buffer/Uint8Array — Node prints as `<Buffer xx xx xx ...>`
                // (lowercase hex bytes separated by single spaces). Buffer
                // headers don't carry a GC header, so this check must happen
                // BEFORE the GC_HEADER_SIZE pointer arithmetic below (which
                // would read garbage one word before the BufferHeader).
                let buf_ptr = ptr as *const crate::buffer::BufferHeader;
                format_buffer_value(buf_ptr)
            } else if (ptr as usize) < 0x100000 {
                // Refs #421: Web Fetch (and other) handles are NaN-boxed
                // POINTER_TAG values whose unboxed payload is a small
                // registry id (1, 2, 3, ...) — NOT a real heap pointer.
                // Reading the GC header at `ptr - 8` would deref unmapped
                // memory and SIGSEGV. Print a placeholder that distinguishes
                // the value from "{}". A future enhancement can look up the
                // specific registry (Request / Response / Headers / Blob /
                // sockets / DB connections / etc.) and format with the type
                // name and key fields the way Node does for those classes.
                "{}".to_string()
            } else {
                // Use GC header to determine the actual type of the object.
                // The GC header is located GC_HEADER_SIZE bytes before the user pointer.
                let gc_header =
                    (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
                let gc_type = (*gc_header).obj_type;

                if gc_type == crate::gc::GC_TYPE_ERROR {
                    // Error object
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    let name_ptr = (*error_ptr).name;
                    let message_ptr = (*error_ptr).message;

                    let name_str = if name_ptr.is_null() {
                        "Error".to_string()
                    } else {
                        let len = (*name_ptr).byte_len as usize;
                        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                    };

                    let message_str = if message_ptr.is_null() {
                        "".to_string()
                    } else {
                        let len = (*message_ptr).byte_len as usize;
                        let data =
                            (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("").to_string()
                    };

                    if message_str.is_empty() {
                        name_str
                    } else {
                        format!("{}: {}", name_str, message_str)
                    }
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
                    let use_multiline = length > 6 || inner.len() + 4 > 76;
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
                        // Non-numeric multi-line: 4 per line, no padding
                        let indent = "  ";
                        let mut row_strs: Vec<String> = parts
                            .chunks(4)
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
                    if let Err(id) = inspect_enter_circular(ptr as usize) {
                        return format!("[Circular *{}]", id);
                    }
                    if depth > inspect_depth_limit() {
                        return inspect_finish_circular(ptr as usize, "[Object]".to_string());
                    }
                    let obj_ptr = ptr as *const crate::object::ObjectHeader;
                    let keys_array = (*obj_ptr).keys_array;

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
                    "Map {}".to_string()
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
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
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
            if let Some((func_ptr, param_count)) =
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
                );
                let ret_jv = crate::value::JSValue::from_bits(ret.to_bits());
                if ret_jv.is_any_string() {
                    return jsvalue_string_content(ret).unwrap_or_default();
                }
                return format_jsvalue(ret, depth);
            }
        }
    }

    let keys_array = (*obj_ptr).keys_array;
    if keys_array.is_null() {
        return "{}".to_string();
    }

    let key_count = crate::array::js_array_length(keys_array) as usize;
    if key_count == 0 {
        return "{}".to_string();
    }

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

    let mut parts: Vec<String> = Vec::with_capacity(key_count);

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

        let is_enumerable = if descriptors_in_use {
            crate::object::get_property_attrs(obj_addr, &key_str)
                .map(|a| a.enumerable())
                .unwrap_or(true)
        } else {
            true
        };
        if !is_enumerable && !show_hidden {
            continue;
        }

        // Get the value
        let value = crate::object::js_object_get_field_f64(obj_ptr, i as u32);
        let value_str = format_jsvalue_for_json(value, depth + 1);

        if is_enumerable {
            parts.push(format!("{}: {}", key_str, value_str));
        } else {
            // Node wraps non-enumerable keys in brackets under showHidden.
            parts.push(format!("[{}]: {}", key_str, value_str));
        }
    }

    // Append symbol-keyed properties last (matches Node's enumeration order:
    // string keys first, then symbol keys). Each entry renders as
    // `[Symbol(<desc>)]: <value>`. By default Node prints enumerable symbol
    // properties even without `showHidden` (they're own enumerable props).
    // Refs #1201.
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
        parts.push(format!("[{}]: {}", sym_label, value_str));
    }

    if parts.is_empty() {
        return "{}".to_string();
    }
    let single_line = format!("{{ {} }}", parts.join(", "));
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
    if !any_child_multiline && single_line.len() <= 72 {
        return single_line;
    }
    let indent = "  ";
    let body = parts
        .iter()
        .map(|p| format!("{}{}", indent, p.replace('\n', "\n  ")))
        .collect::<Vec<_>>()
        .join(",\n");
    format!("{{\n{}\n}}", body)
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
                // First check if this is an Error object
                let object_type = *(ptr as *const u32);
                if object_type == crate::error::OBJECT_TYPE_ERROR {
                    // Format Error as "Error: <message>"
                    let error_ptr = ptr as *const crate::error::ErrorHeader;
                    let name_ptr = (*error_ptr).name;
                    let message_ptr = (*error_ptr).message;

                    let name_str = if name_ptr.is_null() {
                        "Error".to_string()
                    } else {
                        let len = (*name_ptr).byte_len as usize;
                        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("Error").to_string()
                    };

                    let message_str = if message_ptr.is_null() {
                        "".to_string()
                    } else {
                        let len = (*message_ptr).byte_len as usize;
                        let data =
                            (message_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
                        let bytes = std::slice::from_raw_parts(data, len);
                        std::str::from_utf8(bytes).unwrap_or("").to_string()
                    };

                    if message_str.is_empty() {
                        name_str
                    } else {
                        format!("{}: {}", name_str, message_str)
                    }
                } else {
                    // Use GC type header to determine the actual type
                    // instead of heuristic pointer checks which can
                    // misinterpret arrays as objects or vice versa.
                    let gc_header = (ptr as *const u8).sub(crate::gc::GC_HEADER_SIZE)
                        as *const crate::gc::GcHeader;
                    let gc_type = (*gc_header).obj_type;

                    if gc_type == crate::gc::GC_TYPE_ARRAY {
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
                        if let Err(id) = inspect_enter_circular(ptr as usize) {
                            return format!("[Circular *{}]", id);
                        }
                        if depth > inspect_depth_limit() {
                            return inspect_finish_circular(ptr as usize, "[Object]".to_string());
                        }
                        let obj_ptr = ptr as *const crate::object::ObjectHeader;
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
                    } else if gc_type == crate::gc::GC_TYPE_CLOSURE {
                        // Function-valued object fields used to fall through
                        // to the `[object Object]` catch-all below, hiding
                        // both the function name and any user-attached own
                        // properties (e.g. `console.log({ handler: myFn })`
                        // collapsed `myFn` to `[object Object]`). Route
                        // through the same display path `format_jsvalue`'s
                        // own GC_TYPE_CLOSURE branch uses so the registered
                        // function name flows out.
                        format_function_for_console(ptr as *const crate::closure::ClosureHeader)
                    } else {
                        "[object Object]".to_string()
                    }
                }
            }
        } else if jsval.is_int32() {
            jsval.as_int32().to_string()
        } else {
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
            } else if n.fract() == 0.0 && n.abs() < (i64::MAX as f64) {
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
/// #1002: `util.format(fmt, ...args)` / `util.formatWithOptions(opts,
/// fmt, ...args)` native implementation. Codegen bundles the call args
/// into a heap-allocated array (same shape as `js_console_log_spread`)
/// and calls in here; the first element is the format string and the
/// rest are substitution values. Returns a NaN-boxed string.
///
/// Placeholder support mirrors Node's `util.format` for the substrings
/// most callers care about: `%s` (string-coerce), `%d`/`%i` (integer),
/// `%f` (float), `%j` (JSON), `%o`/`%O` (object inspect), `%%` (literal
/// percent). Anything else is left as-is. Trailing args without a
/// matching placeholder are appended space-separated, again matching
/// Node.
///
/// When the first array element isn't a string, Node falls back to
/// space-joining every arg through `util.inspect` — same here, going
/// through `format_jsvalue` for parity with `console.log`.
#[no_mangle]
pub extern "C" fn js_util_format(arr_ptr: *const crate::array::ArrayHeader) -> f64 {
    use crate::value::JSValue;
    // Helper: produce a NaN-boxed string from a Rust `&str`.
    fn boxed_string(s: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
        f64::from_bits(JSValue::string_ptr(ptr).bits())
    }
    // Helper: turn any JS value into its `String(value)` coercion using
    // Perry's existing helper (covers strings, numbers, null/undefined,
    // objects via their .toString protocol).
    unsafe fn jsvalue_as_owned_string(val: f64) -> String {
        let s_ptr = crate::value::js_jsvalue_to_string(val);
        if s_ptr.is_null() {
            return String::new();
        }
        let len = (*s_ptr).byte_len as usize;
        let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        let bs = std::slice::from_raw_parts(data, len);
        std::str::from_utf8(bs).unwrap_or("").to_string()
    }
    if arr_ptr.is_null() {
        return boxed_string("");
    }
    unsafe {
        let length = (*arr_ptr).length as usize;
        let data_ptr = (arr_ptr as *const u8).add(std::mem::size_of::<crate::array::ArrayHeader>())
            as *const f64;

        // No format string → empty result. Node returns "" for
        // `util.format()`.
        if length == 0 {
            return boxed_string("");
        }

        // If arg[0] isn't a string, fall back to space-joining every
        // arg with `format_jsvalue` (matches Node's non-string-first
        // util.format codepath).
        let first = *data_ptr;
        let first_jv = JSValue::from_bits(first.to_bits());
        if !first_jv.is_any_string() {
            let mut parts: Vec<String> = Vec::with_capacity(length);
            for i in 0..length {
                parts.push(format_jsvalue(*data_ptr.add(i), 0));
            }
            return boxed_string(&parts.join(" "));
        }

        // Materialize the format string. Short strings live inline in
        // the NaN-box (top bits set), long strings live in a
        // StringHeader. The unified helper handles both.
        let fmt = jsvalue_as_owned_string(first);
        if length == 1 {
            return boxed_string(&fmt);
        }

        let mut out = String::with_capacity(fmt.len());
        let mut arg_idx: usize = 1;
        let bytes = fmt.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b != b'%' || i + 1 >= bytes.len() {
                out.push(b as char);
                i += 1;
                continue;
            }
            let spec = bytes[i + 1];
            // `%%` → literal `%` (no arg consumed).
            if spec == b'%' {
                out.push('%');
                i += 2;
                continue;
            }
            // Out of args: leave the placeholder untouched (Node does
            // the same — `util.format("%s %s", "x")` prints `"x %s"`).
            if arg_idx >= length {
                out.push('%');
                out.push(spec as char);
                i += 2;
                continue;
            }
            let val = *data_ptr.add(arg_idx);
            arg_idx += 1;
            let jv = JSValue::from_bits(val.to_bits());
            match spec {
                b's' => {
                    out.push_str(&jsvalue_as_owned_string(val));
                }
                b'd' | b'i' => {
                    // Node preserves the BigInt `n` suffix for `%d` / `%i`
                    // (e.g. `util.format("%d", 5n)` → `"5n"`).
                    if jv.is_bigint() {
                        out.push_str(&format_bigint_literal(val));
                    } else {
                        let f = if jv.is_int32() {
                            jv.as_int32() as f64
                        } else if jv.is_any_string()
                            && jsvalue_string_content(val)
                                .map(|s| s.is_empty())
                                .unwrap_or(false)
                        {
                            f64::NAN
                        } else {
                            js_number_coerce(val)
                        };
                        if f.is_nan() {
                            out.push_str("NaN");
                        } else {
                            let t = f.trunc();
                            if t == 0.0 && f.is_sign_negative() {
                                out.push_str("-0");
                            } else {
                                // Integer-truncated, matching Node.
                                out.push_str(&(t as i64).to_string());
                            }
                        }
                    }
                }
                b'f' => {
                    // Node coerces BigInt lossily to Number for `%f`
                    // (`util.format("%f", 5n)` → `"5"`), dropping the `n`.
                    if jv.is_bigint() {
                        let ptr = jv.as_bigint_ptr();
                        let f = if ptr.is_null() {
                            f64::NAN
                        } else {
                            crate::bigint::js_bigint_to_f64(ptr)
                        };
                        if f.is_nan() {
                            out.push_str("NaN");
                        } else {
                            out.push_str(&format_finite_number_js(f));
                        }
                    } else {
                        let f = if jv.is_int32() {
                            jv.as_int32() as f64
                        } else if jv.is_any_string()
                            && jsvalue_string_content(val)
                                .map(|s| s.is_empty())
                                .unwrap_or(false)
                        {
                            f64::NAN
                        } else {
                            js_number_coerce(val)
                        };
                        if f.is_nan() {
                            out.push_str("NaN");
                        } else {
                            out.push_str(&format_finite_number_js(f));
                        }
                    }
                }
                b'j' => {
                    // Real JSON.stringify — string-replace post-processing
                    // of inspect output mangles strings that contain
                    // ", ", ": ", "{ ", or " }".
                    unsafe {
                        let s_ptr = crate::json::js_json_stringify(val, 0);
                        if s_ptr.is_null() {
                            out.push_str("undefined");
                        } else {
                            let len = (*s_ptr).byte_len as usize;
                            let data = (s_ptr as *const u8)
                                .add(std::mem::size_of::<crate::string::StringHeader>());
                            let bytes = std::slice::from_raw_parts(data, len);
                            out.push_str(std::str::from_utf8(bytes).unwrap_or(""));
                        }
                    }
                }
                b'o' => {
                    // Node's `%o` uses an effectively unbounded inspect
                    // depth (showHidden + showProxy with no depth cap on
                    // the typical fixtures used in the parity suite), so
                    // raise the per-thread depth limit just for this call.
                    let _guard = InspectDepthLimitGuard::new(usize::MAX);
                    out.push_str(&format_jsvalue(val, 0));
                }
                b'O' => {
                    // `%O` keeps the default depth cap (2) — matching
                    // Node's `util.inspect` default options.
                    out.push_str(&format_jsvalue(val, 0));
                }
                b'c' => {
                    // Browser/Node console style marker. Consume the CSS
                    // argument but do not emit ANSI styling in the
                    // NO_COLOR parity environment.
                }
                _ => {
                    // Unknown specifier: leave verbatim, don't consume
                    // the arg (Node 22+ behavior — older Node consumed
                    // it; modern behavior is what libraries write
                    // against).
                    out.push('%');
                    out.push(spec as char);
                    arg_idx -= 1;
                }
            }
            i += 2;
        }

        // Append any remaining args separated by spaces, again matching
        // Node: `util.format("hi", "x", "y")` → `"hi x y"`.
        while arg_idx < length {
            out.push(' ');
            out.push_str(&format_jsvalue(*data_ptr.add(arg_idx), 0));
            arg_idx += 1;
        }

        boxed_string(&out)
    }
}

#[no_mangle]
pub extern "C" fn js_util_inspect(value: f64, options: f64) -> f64 {
    // `util.inspect` defaults to `customInspect: true`; an explicit
    // `{ customInspect: false }` opts out and surfaces the hook as a
    // symbol property. Refs #1201.
    let custom_inspect =
        unsafe { super::console::decode_dir_bool_option(options, "customInspect") }.unwrap_or(true);
    let _custom_guard = InspectCustomInspectGuard::new(custom_inspect);
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    let out = if jv.is_any_string() {
        let s = jsvalue_string_content(value).unwrap_or_default();
        format!("'{}'", escape_string(&s))
    } else {
        format_jsvalue(value, 0)
    };
    let ptr = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
    f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
}

#[inline]
fn looks_like_raw_heap_pointer(value: f64) -> bool {
    let bits = value.to_bits();
    if (bits >> 48) >= 0x7FF8 {
        return false;
    }
    let addr = bits as usize;
    (0x1000..0x8000_0000_0000usize).contains(&addr) && addr >= crate::gc::GC_HEADER_SIZE + 0x1000
}

const CLASS_ID_BOXED_NUMBER: u32 = 0xFFFF_0060;
const CLASS_ID_BOXED_STRING: u32 = 0xFFFF_0061;
const CLASS_ID_BOXED_BOOLEAN: u32 = 0xFFFF_0062;

#[inline]
fn boxed_primitive_payload(value: f64) -> Option<(u32, f64)> {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    unsafe {
        let class_id = (*ptr).class_id;
        if !matches!(
            class_id,
            CLASS_ID_BOXED_NUMBER | CLASS_ID_BOXED_STRING | CLASS_ID_BOXED_BOOLEAN
        ) {
            return None;
        }
        let payload = crate::object::js_object_get_field_f64(ptr, 0);
        Some((class_id, payload))
    }
}

#[no_mangle]
pub extern "C" fn js_boxed_number_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_NUMBER, 1);
    // `new Number()` (no args) is spec'd to box +0, not NaN. js_number_coerce
    // would map undefined → NaN, so detect the missing-arg sentinel first.
    let payload = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        0.0
    } else {
        js_number_coerce(value)
    };
    crate::object::js_object_set_field_f64(obj, 0, payload);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_string_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_STRING, 1);
    // `new String()` (no args) is spec'd to box "", not "undefined".
    let ptr = if crate::value::JSValue::from_bits(value.to_bits()).is_undefined() {
        crate::string::js_string_from_bytes(std::ptr::null(), 0)
    } else {
        js_string_coerce(value)
    };
    let boxed = f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits());
    crate::object::js_object_set_field_f64(obj, 0, boxed);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_boxed_boolean_new(value: f64) -> f64 {
    let obj = crate::object::js_object_alloc(CLASS_ID_BOXED_BOOLEAN, 1);
    let boxed =
        f64::from_bits(crate::value::JSValue::bool(crate::value::js_is_truthy(value) != 0).bits());
    crate::object::js_object_set_field_f64(obj, 0, boxed);
    crate::value::js_nanbox_pointer(obj as i64)
}

#[no_mangle]
pub extern "C" fn js_util_is_deep_strict_equal(left: f64, right: f64) -> f64 {
    let left_value = crate::value::JSValue::from_bits(left.to_bits());
    let right_value = crate::value::JSValue::from_bits(right.to_bits());
    let left_boxed = boxed_primitive_payload(left);
    let right_boxed = boxed_primitive_payload(right);
    if left_boxed.is_some() || right_boxed.is_some() {
        let equal = match (left_boxed, right_boxed) {
            (Some((left_class, left_payload)), Some((right_class, right_payload)))
                if left_class == right_class =>
            {
                let payload_equal = js_util_is_deep_strict_equal(left_payload, right_payload);
                crate::value::js_is_truthy(payload_equal) != 0
            }
            _ => false,
        };
        return f64::from_bits(crate::value::JSValue::bool(equal).bits());
    }
    let has_tagged_heap_operand = left_value.is_pointer() || right_value.is_pointer();
    let has_raw_heap_operand =
        looks_like_raw_heap_pointer(left) || looks_like_raw_heap_pointer(right);
    let equal = if has_raw_heap_operand {
        false
    } else if has_tagged_heap_operand {
        format_jsvalue_for_json(left, 0) == format_jsvalue_for_json(right, 0)
    } else {
        crate::value::js_jsvalue_equals(left, right) != 0
            || format_jsvalue_for_json(left, 0) == format_jsvalue_for_json(right, 0)
    };
    f64::from_bits(crate::value::JSValue::bool(equal).bits())
}

#[no_mangle]
pub extern "C" fn js_util_strip_vt_control_characters(value: f64) -> f64 {
    unsafe {
        let s_ptr = crate::value::js_jsvalue_to_string(value);
        let input = if s_ptr.is_null() {
            String::new()
        } else {
            let len = (*s_ptr).byte_len as usize;
            let data = (s_ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            std::str::from_utf8(bytes).unwrap_or("").to_string()
        };
        let mut out = String::with_capacity(input.len());
        let bytes = input.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b {
                let start = i;
                i += 1;
                if i < bytes.len() && bytes[i] == b'[' {
                    i += 1;
                    while i < bytes.len() {
                        let b = bytes[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&b) {
                            break;
                        }
                    }
                    continue;
                } else if i < bytes.len() && bytes[i] == b']' {
                    i += 1;
                    while i < bytes.len() {
                        if bytes[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                    continue;
                }
                out.push_str(&input[start..i]);
            } else {
                // Preserve multi-byte UTF-8 sequences: advance by the
                // full code-point width instead of casting one byte to
                // char (which mangles non-ASCII, e.g. "café" → "cafÃ©").
                let lead = bytes[i];
                let width = if lead < 0x80 {
                    1
                } else if lead < 0xc0 {
                    1 // stray continuation byte; copy verbatim
                } else if lead < 0xe0 {
                    2
                } else if lead < 0xf0 {
                    3
                } else {
                    4
                };
                let end = (i + width).min(bytes.len());
                out.push_str(std::str::from_utf8(&bytes[i..end]).unwrap_or(""));
                i = end;
            }
        }
        let ptr = crate::string::js_string_from_bytes(out.as_ptr(), out.len() as u32);
        f64::from_bits(crate::value::JSValue::string_ptr(ptr).bits())
    }
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
