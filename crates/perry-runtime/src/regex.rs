//! RegExp runtime support for Perry
//!
//! Provides JavaScript-compatible regular expression operations using the Rust regex crate.
//! RegExp objects are heap-allocated and store the compiled pattern and flags.

use regex::Regex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ptr;
use std::sync::Arc;

use crate::array::ArrayHeader;
use crate::string::StringHeader;
use crate::value::js_nanbox_string;

use crate::object::ObjectHeader;

mod escape;
mod grammar;
mod match_all;
mod replace_expand;
mod replace_fn;
pub use escape::js_regexp_escape;
use grammar::{has_invalid_repeated_quantifier, js_regex_to_rust};
pub use match_all::{
    dispatch_regexp_string_iterator_method, js_string_match_all, js_string_match_all_value,
    REGEXP_STRING_ITERATOR_CLASS_ID,
};
use replace_expand::{expand_js_replacement, replace_regex_fn_fancy};
pub use replace_expand::{
    js_string_replace_all_regex_fn, js_string_replace_regex_fn, js_string_replace_regex_named,
};
use replace_fn::call_replace_callback;
pub use replace_fn::{
    js_string_replace_all_string, js_string_replace_all_string_fn, js_string_replace_string,
    js_string_replace_string_fn,
};

thread_local! {
    /// Last exec result metadata: (index, groups_object_ptr)
    /// Stored per-thread so that `m.index` and `m.groups` can retrieve them
    /// after the exec call.
    static LAST_EXEC_INDEX: RefCell<f64> = const { RefCell::new(0.0) };
    static LAST_EXEC_GROUPS: RefCell<*mut ObjectHeader> = const { RefCell::new(ptr::null_mut()) };

    /// Set of all RegExpHeader pointers ever allocated in this thread.
    /// Used by callers (e.g. `js_string_split`) to distinguish a regex
    /// delimiter from a string delimiter when the codegen can't tell
    /// statically. Pointers are never removed; RegExpHeader is backed by
    /// `gc_malloc` but headers are effectively permanent in practice, and
    /// even if a header is freed, subsequent lookups will simply miss —
    /// the worst outcome is that a stale regex is treated as a string
    /// (safe) rather than the other way around (segfault).
    static REGEX_POINTERS: RefCell<HashSet<usize>> = RefCell::new(HashSet::new());

    /// Issue #637: Owned copies of pattern and flags strings keyed by
    /// the RegExpHeader pointer. The header's `pattern_ptr` / `flags_ptr`
    /// fields hold raw `*const StringHeader` pointers to the input
    /// strings — when those inputs are temporaries (e.g. the result of
    /// a template-literal expression `\`^${p}\``), the GC frees them
    /// after the function call returns and subsequent `.source` /
    /// `.flags` reads dereference dangling memory. We side-table an
    /// owned `String` copy at construction time; readers prefer this
    /// over `pattern_ptr` whenever an entry exists.
    static REGEX_SOURCE_TABLE: RefCell<HashMap<usize, (String, String)>> = RefCell::new(HashMap::new());
}

/// Check whether `ptr` is a RegExpHeader pointer that was allocated in
/// this thread. Called by `js_string_split` to detect the `s.split(re)`
/// case without a separate runtime FFI entry point.
pub(crate) fn is_regex_pointer(ptr: *const u8) -> bool {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return false;
    }
    REGEX_POINTERS.with(|s| s.borrow().contains(&(ptr as usize)))
}

thread_local! {
    /// Cache of compiled regex objects, keyed by (pattern, flags).
    static REGEX_CACHE: RefCell<HashMap<(String, String), Arc<Regex>>> = RefCell::new(HashMap::new());
    /// Fancy-regex fallback cache for patterns with lookbehind/lookahead.
    static FANCY_CACHE: RefCell<HashMap<(String, String), Arc<fancy_regex::Regex>>> = RefCell::new(HashMap::new());
}

fn get_or_compile_regex(pattern: &str, flags: &str) -> Arc<Regex> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(re) = cache.get(&(pattern.to_string(), flags.to_string())) {
            return re.clone();
        }
        // Translate JS regex to Rust-compatible pattern
        let translated = js_regex_to_rust(pattern);
        let case_insensitive = flags.contains('i');
        let multiline = flags.contains('m');
        // #2828: the `s` (dotAll) flag maps directly onto the Rust `regex`
        // crate's `(?s)` inline mode, so `.` matches newlines.
        let dot_all = flags.contains('s');
        let regex_pattern = if case_insensitive || multiline || dot_all {
            let mut prefix = String::from("(?");
            if case_insensitive {
                prefix.push('i');
            }
            if multiline {
                prefix.push('m');
            }
            if dot_all {
                prefix.push('s');
            }
            prefix.push(')');
            format!("{}{}", prefix, translated)
        } else {
            translated
        };
        let regex = match Regex::new(&regex_pattern) {
            Ok(re) => re,
            Err(_) => {
                // Pattern has features regex crate doesn't support
                // (lookbehind, lookahead). Try fancy-regex which supports
                // the full JS regex feature set, and if it compiles, wrap
                // the result via a find-and-replace approach at the exec
                // call sites. For now, store a never-matching pattern so
                // existing callers don't crash — the fancy-regex fallback
                // is handled in js_regexp_exec_fancy below.
                FANCY_CACHE.with(|fc| {
                    if let Ok(fre) = fancy_regex::Regex::new(&regex_pattern) {
                        fc.borrow_mut().insert(
                            (pattern.to_string(), flags.to_string()),
                            std::sync::Arc::new(fre),
                        );
                    }
                });
                Regex::new(r"[^\s\S]").unwrap()
            }
        };
        let arc = Arc::new(regex);
        cache.insert((pattern.to_string(), flags.to_string()), arc.clone());
        arc
    })
}

/// Header for heap-allocated RegExp objects
#[repr(C)]
pub struct RegExpHeader {
    /// Pointer to the compiled Regex object (boxed)
    regex_ptr: *mut Regex,
    /// Original pattern string (for debugging/serialization)
    pattern_ptr: *const StringHeader,
    /// Flags string (e.g., "gi" for global+ignoreCase)
    flags_ptr: *const StringHeader,
    /// Cached flags for quick access
    pub case_insensitive: bool,
    pub global: bool,
    pub multiline: bool,
    /// #2828: additional observable flags. `sticky`/`unicode`/`has_indices`
    /// are exposed via getters (matching behavior is scoped — see notes in
    /// `js_regexp_new`); `dot_all` IS honored at compile time via `(?s)`.
    pub sticky: bool,
    pub dot_all: bool,
    pub unicode: bool,
    pub has_indices: bool,
    /// `lastIndex` is a writable data property holding an *arbitrary* JSValue
    /// (spec: `Set(R, "lastIndex", v)` with no coercion on write). Stored as the
    /// raw NaN-boxed bits; `exec`/`test` apply `ToLength` on read to derive the
    /// match offset. Initialized to the number `0`.
    pub last_index: u64,
}

/// `ToLength(Get(R, "lastIndex"))` → a non-negative integer match offset. The
/// stored value may be any JSValue (e.g. `re.lastIndex = { valueOf() {…} }`), so
/// coerce via `ToNumber` (which invokes `valueOf`/`toString`), then `ToInteger`,
/// clamped to ≥ 0.
pub(crate) fn regex_last_index_offset(re: *const RegExpHeader) -> usize {
    let stored = f64::from_bits(unsafe { (*re).last_index });
    let n = crate::builtins::js_number_coerce(stored);
    if n.is_nan() || n <= 0.0 {
        0
    } else {
        n.floor() as usize
    }
}

#[inline]
fn store_last_index_number(re: *mut RegExpHeader, n: usize) {
    unsafe {
        (*re).last_index = crate::value::JSValue::number(n as f64).bits();
    }
}

/// Check if a pointer is valid (not null and not a small invalid value from bad NaN-unboxing)
#[inline]
fn is_valid_ptr<T>(p: *const T) -> bool {
    !p.is_null() && (p as usize) >= 0x1000
}

/// Check if a RegExpHeader pointer is legitimate — it must point to a
/// header we allocated via `js_regexp_new` (tracked in REGEX_POINTERS).
/// The LLVM backend's `new RegExp(pat, flags)` currently falls through
/// to the generic `lower_new` path which allocates an empty object and
/// NaN-boxes it as a regex; subsequent `.exec()` / `.test()` calls would
/// read garbage from that object if we didn't gate them on this check.
#[inline]
fn is_valid_regex_ptr(p: *const RegExpHeader) -> bool {
    if !is_valid_ptr(p) {
        return false;
    }
    REGEX_POINTERS.with(|s| s.borrow().contains(&(p as usize)))
}

/// Public: is `addr` a RegExpHeader we allocated via `js_regexp_new`?
/// Used by the console/`util.inspect` formatter to print regex literals
/// as `/source/flags` instead of `{}` (they're GC_TYPE_OBJECT allocations
/// with no enumerable string keys). Registry-gated so a generic object
/// is never mis-read as a RegExpHeader.
pub fn is_registered_regex(addr: usize) -> bool {
    REGEX_POINTERS.with(|s| s.borrow().contains(&addr))
}

/// Internal helper: Get string data from StringHeader
fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8_unchecked(bytes)
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
fn js_string_from_str(s: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

fn throw_replace_all_non_global_regex() -> ! {
    let message = b"String.prototype.replaceAll called with a non-global RegExp argument";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_match_all_non_global_regex() -> ! {
    let message = b"String.prototype.matchAll called with a non-global RegExp argument";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn set_exec_array_metadata(arr: *mut ArrayHeader, input: &str, index: f64) {
    if arr.is_null() {
        return;
    }
    let index_key = js_string_from_str("index");
    crate::array::js_array_set_string_key(arr, index_key, index);

    let input_key = js_string_from_str("input");
    let input_str = js_string_from_str(input);
    let input_value = js_nanbox_string(input_str as i64);
    crate::array::js_array_set_string_key(arr, input_key, input_value);
}

/// Attach the `groups` own property to a regex match-result array.
///
/// Mirrors `set_exec_array_metadata` for `index`/`input`: the result of
/// `regex.exec(s)` / `s.match(regex)` carries `groups` as a real own property
/// so reads stay correct under aliasing and interleaved matches — a stored
/// `m.groups` survives a later `re2.exec(...)`, instead of resolving through a
/// single most-recent-match thread-local (`LAST_EXEC_GROUPS`). Per ECMA-262
/// RegExpBuiltinExec, `groups` is the named-capture object when the pattern
/// has named groups, else `undefined`.
fn set_exec_array_groups(arr: *mut ArrayHeader, groups_obj: *mut ObjectHeader) {
    if arr.is_null() {
        return;
    }
    let groups_key = js_string_from_str("groups");
    let value = if groups_obj.is_null() {
        f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
    } else {
        crate::value::js_nanbox_pointer(groups_obj as i64)
    };
    crate::array::js_array_set_string_key(arr, groups_key, value);
}

fn char_index_to_byte(s: &str, char_index: usize) -> usize {
    if char_index == 0 {
        return 0;
    }
    for (idx, (byte, _)) in s.char_indices().enumerate() {
        if idx == char_index {
            return byte;
        }
    }
    s.len()
}

fn byte_index_to_char_index(s: &str, byte_index: usize) -> f64 {
    s[..byte_index.min(s.len())].chars().count() as f64
}

#[inline]
fn ensure_replace_all_regex_global(re: *const RegExpHeader) {
    unsafe {
        if !(*re).global {
            throw_replace_all_non_global_regex();
        }
    }
}

/// Throw a `SyntaxError` with the given message and never return.
fn throw_regexp_syntax_error(message: &str) -> ! {
    let msg = js_string_from_str(message);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// #2829: validate a RegExp flags string the way the spec's
/// `RegExpInitialize` does — each flag must be one of `dgimsuvy` and must not
/// repeat. Returns the flags in canonical (sorted) order, or throws a
/// `SyntaxError` mirroring Node's "Invalid flags supplied to RegExp
/// constructor '<flags>'" message.
///
/// Note: the `v` flag (unicodeSets) is accepted as a valid flag for parity but
/// its set-notation matching semantics are not implemented (the regex crate
/// has no equivalent); it behaves like an ordinary unicode pattern.
fn validate_and_canonicalize_flags(flags: &str) -> String {
    // Spec order of the flag bits: d g i m s u v y.
    const FLAG_ORDER: &[char] = &['d', 'g', 'i', 'm', 's', 'u', 'v', 'y'];
    let mut seen = [false; 8];
    for ch in flags.chars() {
        match FLAG_ORDER.iter().position(|&f| f == ch) {
            Some(idx) => {
                if seen[idx] {
                    throw_regexp_syntax_error(&format!(
                        "Invalid flags supplied to RegExp constructor '{}'",
                        flags
                    ));
                }
                seen[idx] = true;
            }
            None => {
                throw_regexp_syntax_error(&format!(
                    "Invalid flags supplied to RegExp constructor '{}'",
                    flags
                ));
            }
        }
    }
    FLAG_ORDER
        .iter()
        .enumerate()
        .filter(|(i, _)| seen[*i])
        .map(|(_, c)| *c)
        .collect()
}

/// Create a new RegExp from pattern and flags strings
/// Returns a pointer to RegExpHeader
///
/// Uses the thread-local REGEX_CACHE so repeated regex literals (e.g. in a
/// loop) reuse the same compiled Regex instead of leaking a fresh one each
/// time. The raw pointer stored in RegExpHeader is kept alive by the cache.
#[no_mangle]
pub extern "C" fn js_regexp_new(
    pattern: *const StringHeader,
    flags: *const StringHeader,
) -> *mut RegExpHeader {
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };
    let raw_flags_str = if is_valid_ptr(flags) {
        string_as_str(flags)
    } else {
        ""
    };

    // #2829: reject duplicate/unknown flags (SyntaxError) and store the
    // canonical sorted form so `.flags` reflects Node's ordering.
    let canonical_flags = validate_and_canonicalize_flags(raw_flags_str);
    let flags_str = canonical_flags.as_str();

    let case_insensitive = flags_str.contains('i');
    let global = flags_str.contains('g');
    let multiline = flags_str.contains('m');
    let sticky = flags_str.contains('y');
    let dot_all = flags_str.contains('s');
    let unicode = flags_str.contains('u') || flags_str.contains('v');
    let has_indices = flags_str.contains('d');

    // #2829: reject invalid pattern syntax with a SyntaxError. A pattern the
    // `regex` crate rejects is only a real error if `fancy-regex` (which
    // covers the full JS feature set: lookbehind/lookahead/backreferences)
    // ALSO rejects it — otherwise it is a valid JS pattern we route through
    // the fancy fallback. `get_or_compile_regex` populates FANCY_CACHE when
    // the regex crate fails but fancy-regex succeeds; check both here.
    {
        if has_invalid_repeated_quantifier(pattern_str) {
            throw_regexp_syntax_error(&format!(
                "Invalid regular expression: /{}/: invalid pattern",
                pattern_str
            ));
        }
        let translated = js_regex_to_rust(pattern_str);
        if regex::Regex::new(&translated).is_err() && fancy_regex::Regex::new(&translated).is_err()
        {
            throw_regexp_syntax_error(&format!(
                "Invalid regular expression: /{}/: invalid pattern",
                pattern_str
            ));
        }
    }

    // Get or compile the regex from the cache. The returned Arc is stored
    // in the cache indefinitely, so the raw pointer we extract stays valid
    // for the lifetime of the process.
    let arc = get_or_compile_regex(pattern_str, flags_str);
    let regex_ptr = Arc::as_ptr(&arc) as *mut Regex;

    // Allocate the header via gc_malloc so it's tracked by the GC and gets
    // freed when no longer referenced. Previously this used raw alloc() and
    // leaked every header, which was a 64-byte-per-call leak on top of the
    // (now-fixed) regex object leak.
    let header_size = std::mem::size_of::<RegExpHeader>();
    // Materialize the canonical flags into a fresh StringHeader so that
    // `flags_ptr`-keyed lookups (FANCY_CACHE, lookup_fancy_regex) and the
    // GC-survivable source table all agree on the canonical form, and the
    // header never holds the caller's possibly-temporary input flags.
    let canonical_flags_ptr = js_string_from_str(flags_str);
    unsafe {
        let raw = crate::gc::gc_malloc(header_size, crate::gc::GC_TYPE_OBJECT);
        if raw.is_null() {
            panic!("Failed to allocate RegExp");
        }
        let ptr = raw as *mut RegExpHeader;

        (*ptr).regex_ptr = regex_ptr;
        (*ptr).pattern_ptr = pattern;
        (*ptr).flags_ptr = canonical_flags_ptr;
        (*ptr).case_insensitive = case_insensitive;
        (*ptr).global = global;
        (*ptr).multiline = multiline;
        (*ptr).sticky = sticky;
        (*ptr).dot_all = dot_all;
        (*ptr).unicode = unicode;
        (*ptr).has_indices = has_indices;
        (*ptr).last_index = crate::value::JSValue::number(0.0).bits();

        // Record the pointer so that js_string_split can detect
        // `s.split(regex)` without a dedicated runtime decl.
        REGEX_POINTERS.with(|s| {
            s.borrow_mut().insert(ptr as usize);
        });

        // Issue #637: side-table owned copies of pattern + flags so
        // `.source` / `.flags` survive GC of the input StringHeaders.
        REGEX_SOURCE_TABLE.with(|t| {
            t.borrow_mut().insert(
                ptr as usize,
                (pattern_str.to_string(), flags_str.to_string()),
            );
        });

        ptr
    }
}

/// ECMA-262 RegExp constructor (`new RegExp(pattern, flags)`), spec 22.2.4.
/// Handles every argument shape the string/string `js_regexp_new` cannot:
///
///   * `pattern` is a RegExp → reuse its `[[OriginalSource]]`; if `flags` is
///     `undefined`, reuse its `[[OriginalFlags]]`, else `ToString(flags)`.
///   * `pattern` is `undefined` → empty source.
///   * `pattern` is anything else → `ToString(pattern)`.
///   * `flags` is `undefined` → empty (unless inherited from a RegExp pattern);
///     anything else → `ToString(flags)` (so `{}` becomes `"[object Object]"`,
///     which `js_regexp_new` then rejects with a SyntaxError).
///
/// `ToString` runs through the coercing method path so a throwing
/// `toString`/`valueOf` propagates.
#[no_mangle]
pub extern "C" fn js_regexp_construct(pattern: f64, flags: f64) -> *mut RegExpHeader {
    let pv = crate::value::JSValue::from_bits(pattern.to_bits());
    let fv = crate::value::JSValue::from_bits(flags.to_bits());
    let flags_undef = fv.is_undefined();

    let pattern_is_regex = pv.is_pointer() && is_registered_regex(pv.as_pointer::<u8>() as usize);

    let (source_string, inherited_flags) = if pattern_is_regex {
        let re = pv.as_pointer::<RegExpHeader>();
        let entry = REGEX_SOURCE_TABLE.with(|t| t.borrow().get(&(re as usize)).cloned());
        match entry {
            Some((pat, fl)) => (pat, Some(fl)),
            None => (String::new(), Some(String::new())),
        }
    } else if pv.is_undefined() {
        (String::new(), None)
    } else {
        let s = crate::value::js_jsvalue_to_string_coerce(pattern);
        (
            if is_valid_ptr(s) {
                string_as_str(s).to_string()
            } else {
                String::new()
            },
            None,
        )
    };

    let flags_string = if flags_undef {
        inherited_flags.unwrap_or_default()
    } else {
        let s = crate::value::js_jsvalue_to_string_coerce(flags);
        if is_valid_ptr(s) {
            string_as_str(s).to_string()
        } else {
            String::new()
        }
    };

    let pat_ptr = js_string_from_str(&source_string);
    let flags_ptr = js_string_from_str(&flags_string);
    js_regexp_new(pat_ptr, flags_ptr)
}

/// Test if a string matches the regex pattern
/// regex.test(string) -> boolean
#[no_mangle]
pub extern "C" fn js_regexp_test(re: *const RegExpHeader, s: *const StringHeader) -> i32 {
    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        return 0;
    }

    let str_data = string_as_str(s);

    unsafe {
        // For global/sticky regexes `test` is stateful — it must consult and
        // advance `lastIndex` (and anchor for sticky) exactly like `exec`. Route
        // through `exec` so the lastIndex bookkeeping stays in one place; `test`
        // just reports whether a match was produced.
        if (*re).global || (*re).sticky {
            let arr = js_regexp_exec(re as *mut RegExpHeader, s);
            return if arr.is_null() { 0 } else { 1 };
        }

        if let Some(fre) = lookup_fancy_regex(re) {
            return match fre.is_match(str_data) {
                Ok(true) => 1,
                Ok(false) | Err(_) => 0,
            };
        }

        let regex = &*(*re).regex_ptr;
        if regex.is_match(str_data) {
            1
        } else {
            0
        }
    }
}

/// Look up a fancy-regex fallback for the given header, if one was
/// registered at compile-time because the `regex` crate rejected the
/// pattern (backreferences, lookbehind, etc.).
fn lookup_fancy_regex(re: *const RegExpHeader) -> Option<Arc<fancy_regex::Regex>> {
    unsafe {
        let pat = string_as_str((*re).pattern_ptr);
        let flags_str = string_as_str((*re).flags_ptr);
        FANCY_CACHE.with(|fc| {
            fc.borrow()
                .get(&(pat.to_string(), flags_str.to_string()))
                .cloned()
        })
    }
}

/// Coerce a `String.prototype.search`/`match` argument into a RegExp
/// (ECMA-262 §22.1.3.12 / §22.1.3.20 → `RegExpCreate`). A RegExp value passes
/// through unchanged; anything else builds a fresh regex whose source pattern
/// is `ToString(arg)` (running user `toString`/`valueOf`, which may throw),
/// with `undefined` mapped to the empty pattern (the `/(?:)/` regex that
/// matches at index 0). Flags default to none.
fn coerce_search_arg_to_regex(arg: f64) -> *const RegExpHeader {
    let jv = crate::value::JSValue::from_bits(arg.to_bits());
    if jv.is_pointer() {
        let p = crate::value::js_nanbox_get_pointer(arg) as *const u8;
        if is_regex_pointer(p) {
            return p as *const RegExpHeader;
        }
    }
    // `undefined` → empty pattern. Build a real empty `StringHeader` (NOT a
    // null pointer): the resulting RegExp header's `pattern_ptr` is later
    // dereferenced by `js_string_match`'s `lookup_fancy_regex`
    // (`string_as_str((*re).pattern_ptr)`), which would SIGSEGV on null.
    let src: *const StringHeader = if jv.is_undefined() {
        crate::string::js_string_from_str("") as *const StringHeader
    } else {
        crate::builtins::js_string_coerce(arg) as *const StringHeader
    };
    // `flags` may be read the same way; pass an empty header rather than null.
    let flags = crate::string::js_string_from_str("") as *const StringHeader;
    js_regexp_new(src, flags)
}

/// `String.prototype.search(regexp)` (ECMA-262 §22.1.3.12) with full argument
/// coercion: a non-RegExp arg is turned into `RegExpCreate(ToString(arg))`
/// (so `"x".search("pat")`, `.search(undefined)`, and `.search({toString})`
/// all work). `s` is the already-`ToString`-coerced `this`.
#[no_mangle]
pub extern "C" fn js_string_search_value(s: *const StringHeader, arg: f64) -> i32 {
    // Root the receiver across the (possibly allocating / GC-triggering)
    // argument coercion so a moving collector can't dangle `s`.
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let re = coerce_search_arg_to_regex(arg);
    let s = s_handle.get_raw_const_ptr::<StringHeader>();
    js_string_search_regex(s, re)
}

/// `String.prototype.match(regexp)` (ECMA-262 §22.1.3.11) with full argument
/// coercion (see [`js_string_search_value`]). Returns the match array pointer,
/// or null on no match.
#[no_mangle]
pub extern "C" fn js_string_match_value(s: *const StringHeader, arg: f64) -> *mut ArrayHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let s_handle = scope.root_string_ptr(s);
    let re = coerce_search_arg_to_regex(arg);
    let s = s_handle.get_raw_const_ptr::<StringHeader>();
    js_string_match(s, re)
}

/// Find matches in a string
/// string.match(regex) -> string[] | null (returns array pointer, null if no match)
#[no_mangle]
pub extern "C" fn js_string_match(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        return ptr::null_mut();
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;

        // If this regex couldn't be compiled by the `regex` crate (e.g.
        // backreferences like `(\w)\1*`, used by date-fns' format token
        // regex), `get_or_compile_regex` substituted a never-match
        // `[^\s\S]` placeholder and stashed the real pattern in
        // `FANCY_CACHE`. Route through fancy-regex so `.match()` returns
        // real results instead of always-null.
        if let Some(fre) = lookup_fancy_regex(re) {
            if global {
                // Collect all non-overlapping matches via fancy-regex's
                // find_iter. Mirrors the `regex` crate global path below.
                let mut matches: Vec<String> = Vec::new();
                let mut iter = fre.find_iter(str_data);
                while let Some(Ok(m)) = iter.next() {
                    matches.push(m.as_str().to_string());
                }
                if matches.is_empty() {
                    return ptr::null_mut();
                }
                let arr = crate::array::js_array_alloc(matches.len() as u32);
                let scope = crate::gc::RuntimeHandleScope::new();
                let arr_handle = scope.root_raw_mut_ptr(arr);
                (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = matches.len() as u32;
                for (i, m) in matches.iter().enumerate() {
                    let str_ptr = js_string_from_str(m);
                    let nanboxed = js_nanbox_string(str_ptr as i64);
                    let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                    // GC_STORE_AUDIT(BARRIERED): regex match array slot uses the shared array slot-store helper.
                    crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                }
                return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            } else {
                // Non-global: first match + capture groups (parallels the
                // standard-regex non-global branch below).
                match fre.captures(str_data) {
                    Ok(Some(caps)) => {
                        let arr = crate::array::js_array_alloc(caps.len() as u32);
                        let scope = crate::gc::RuntimeHandleScope::new();
                        let arr_handle = scope.root_raw_mut_ptr(arr);
                        (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;
                        for i in 0..caps.len() {
                            if let Some(m) = caps.get(i) {
                                let str_ptr = js_string_from_str(m.as_str());
                                let nanboxed = js_nanbox_string(str_ptr as i64);
                                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                                // GC_STORE_AUDIT(BARRIERED): regex capture array slot uses the shared array slot-store helper.
                                crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                            } else {
                                let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
                                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                                // GC_STORE_AUDIT(BARRIERED): regex unmatched capture slot uses the shared array slot-store helper.
                                crate::array::store_array_slot(arr, i, undefined.to_bits());
                            }
                        }
                        // Attach .index / .input as real own properties.
                        let match_char_offset = caps
                            .get(0)
                            .map(|m| str_data[..m.start()].chars().count())
                            .unwrap_or(0);
                        set_exec_array_metadata(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            str_data,
                            match_char_offset as f64,
                        );
                        // Extract named-capture groups through the fancy path
                        // (fancy-regex exposes `capture_names()` just like the
                        // `regex` crate), so `s.match(/(?<=x)(?<y>\d+)/).groups`
                        // works for lookbehind+named patterns.
                        let groups_obj = build_fancy_groups(&fre, &caps, &scope);
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups_obj);
                        set_exec_array_groups(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            groups_obj,
                        );
                        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                    }
                    _ => {
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                        return ptr::null_mut();
                    }
                }
            }
        }

        if global {
            // Global flag: return all matches
            let matches: Vec<&str> = regex.find_iter(str_data).map(|m| m.as_str()).collect();

            if matches.is_empty() {
                return ptr::null_mut();
            }

            // Create array of string pointers
            let arr = crate::array::js_array_alloc(matches.len() as u32);
            let scope = crate::gc::RuntimeHandleScope::new();
            let arr_handle = scope.root_raw_mut_ptr(arr);
            (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = matches.len() as u32;

            for (i, m) in matches.iter().enumerate() {
                let str_ptr = js_string_from_str(m);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                // GC_STORE_AUDIT(BARRIERED): regex global match array slot uses the shared array slot-store helper.
                crate::array::store_array_slot(arr, i, nanboxed.to_bits());
            }

            arr_handle.get_raw_mut_ptr::<ArrayHeader>()
        } else {
            // Non-global: return first match only (or with capture groups)
            match regex.captures(str_data) {
                Some(caps) => {
                    // Return array with full match and capture groups
                    let arr = crate::array::js_array_alloc(caps.len() as u32);
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let arr_handle = scope.root_raw_mut_ptr(arr);
                    (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

                    for (i, cap) in caps.iter().enumerate() {
                        if let Some(m) = cap {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                            // GC_STORE_AUDIT(BARRIERED): regex capture array slot uses the shared array slot-store helper.
                            crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                        } else {
                            // Undefined capture group - store as undefined (TAG_UNDEFINED = 0x7FFC_0000_0000_0001)
                            let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
                            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                            // GC_STORE_AUDIT(BARRIERED): regex unmatched capture slot uses the shared array slot-store helper.
                            crate::array::store_array_slot(arr, i, undefined.to_bits());
                        }
                    }

                    // Attach .index / .input as real own properties (mirrors
                    // js_regexp_exec) so they survive aliasing and a later match
                    // on another regex, instead of a most-recent-match thread-local.
                    let match_char_offset = caps
                        .get(0)
                        .map(|m| str_data[..m.start()].chars().count())
                        .unwrap_or(0);
                    set_exec_array_metadata(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        str_data,
                        match_char_offset as f64,
                    );

                    // Build groups object for named captures (same shape as
                    // `regex.exec(str)` does in `js_regexp_exec`). Stored in
                    // `LAST_EXEC_GROUPS` thread-local so the HIR fold for
                    // `result.groups` (extended in lower.rs::is_regex_exec_init
                    // to also recognize `str.match(regex)` results) reads it
                    // via the existing `Expr::RegExpExecGroups` codegen path.
                    // Same caveats as exec()'s thread-local: only the most
                    // recent match's groups are stashed, so `m1.groups` after
                    // an intervening `m2 = ...match(...)` reads m2's groups —
                    // acceptable for the common inline `m.groups.x` pattern.
                    let group_names: Vec<(&str, Option<regex::Match>)> = regex
                        .capture_names()
                        .enumerate()
                        .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
                        .collect();
                    if !group_names.is_empty() {
                        // Use the by-name setter (and a plain `js_object_alloc`)
                        // so each match's groups object grows its own shape from
                        // its own keys. Pre-fix this took the
                        // `js_object_alloc_with_shape(shape_id=const, ...)` path
                        // — every match's groups object collapsed to the same
                        // interned shape, so a later match with different named
                        // captures inherited the prior call's key names (e.g.
                        // `.match(/(?<year>...)/)` followed by
                        // `.match(/(?<id>...)/)` made the second result expose
                        // `.year` instead of `.id`).
                        let groups_obj = crate::object::js_object_alloc(0, 0);
                        let groups_handle = scope.root_raw_mut_ptr(groups_obj);
                        for (name, m) in &group_names {
                            let val = if let Some(m) = m {
                                let str_ptr = js_string_from_str(m.as_str());
                                js_nanbox_string(str_ptr as i64)
                            } else {
                                f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
                            };
                            let key_ptr = crate::string::js_string_from_bytes(
                                name.as_ptr(),
                                name.len() as u32,
                            );
                            let groups_obj =
                                groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                            crate::object::js_object_set_field_by_name(groups_obj, key_ptr, val);
                        }
                        LAST_EXEC_GROUPS.with(|g| {
                            *g.borrow_mut() =
                                groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>()
                        });
                        set_exec_array_groups(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
                        );
                    } else {
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                        set_exec_array_groups(
                            arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                            ptr::null_mut(),
                        );
                    }

                    arr_handle.get_raw_mut_ptr::<ArrayHeader>()
                }
                None => {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                    ptr::null_mut()
                }
            }
        }
    }
}

/// Replace matches in a string
/// Expand a JS replacement string against one match, supporting the full set

/// Fancy-regex twin of [`expand_js_replacement`]. The two `Captures` types
/// (`regex::Captures` / `fancy_regex::Captures`) expose the same surface used
/// here — `get(0)`, `len()`, `get(n)`, `name(s)`, `Match::{as_str,start,end}` —
/// so the body is a deliberate duplicate of the standard expander with the
/// capture type swapped, mirroring the `replace_regex_fn_fancy` ↔
/// `js_string_replace_regex_fn` pairing already in this file. Used so a pattern
/// the `regex` crate can't compile (lookbehind/backreferences) still gets full
/// `$1`/`$<name>`/`$&`/`` $` ``/`$'`/`$$` substitution.
fn expand_js_replacement_fancy(
    repl: &str,
    caps: &fancy_regex::Captures,
    subject: &str,
    has_named_groups: bool,
) -> String {
    let m0 = match caps.get(0) {
        Some(m) => m,
        None => return String::new(),
    };
    let (mstart, mend) = (m0.start(), m0.end());
    let ngroups = caps.len();
    let b = repl.as_bytes();
    let mut out = String::with_capacity(repl.len() + 16);
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            let start = i;
            while i < b.len() && b[i] != b'$' {
                i += 1;
            }
            out.push_str(&repl[start..i]);
            continue;
        }
        if i + 1 >= b.len() {
            out.push('$');
            i += 1;
            continue;
        }
        match b[i + 1] {
            b'$' => {
                out.push('$');
                i += 2;
            }
            b'&' => {
                out.push_str(&subject[mstart..mend]);
                i += 2;
            }
            b'`' => {
                out.push_str(&subject[..mstart]);
                i += 2;
            }
            b'\'' => {
                out.push_str(&subject[mend..]);
                i += 2;
            }
            b'0'..=b'9' => {
                let d1 = (b[i + 1] - b'0') as usize;
                let (group, consumed) = if i + 2 < b.len() && b[i + 2].is_ascii_digit() {
                    let two = d1 * 10 + (b[i + 2] - b'0') as usize;
                    if two >= 1 && two < ngroups {
                        (Some(two), 2)
                    } else if d1 >= 1 && d1 < ngroups {
                        (Some(d1), 1)
                    } else {
                        (None, 0)
                    }
                } else if d1 >= 1 && d1 < ngroups {
                    (Some(d1), 1)
                } else {
                    (None, 0)
                };
                match group {
                    Some(g) => {
                        if let Some(m) = caps.get(g) {
                            out.push_str(m.as_str());
                        }
                        i += 1 + consumed;
                    }
                    None => {
                        out.push('$');
                        i += 1;
                    }
                }
            }
            b'<' => {
                if has_named_groups {
                    if let Some(rel) = repl[i + 2..].find('>') {
                        let name = &repl[i + 2..i + 2 + rel];
                        if let Some(m) = caps.name(name) {
                            out.push_str(m.as_str());
                        }
                        i += 2 + rel + 1;
                    } else {
                        out.push('$');
                        i += 1;
                    }
                } else {
                    out.push('$');
                    i += 1;
                }
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

/// Build a named-capture `groups` object from a fancy-regex match, or return
/// null when the pattern declares no named capture groups. Mirrors the
/// named-group construction in the standard-engine `js_regexp_exec` path
/// (fresh per-result object + by-name setters so each match grows its own
/// shape). The returned object must be stored into a GC-visible slot by the
/// caller immediately; it is rooted via `scope` until then.
pub(crate) unsafe fn build_fancy_groups(
    fre: &fancy_regex::Regex,
    caps: &fancy_regex::Captures,
    scope: &crate::gc::RuntimeHandleScope,
) -> *mut ObjectHeader {
    let group_names: Vec<(&str, Option<fancy_regex::Match>)> = fre
        .capture_names()
        .enumerate()
        .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
        .collect();
    if group_names.is_empty() {
        return ptr::null_mut();
    }
    let groups_obj = crate::object::js_object_alloc(0, 0);
    let groups_handle = scope.root_raw_mut_ptr(groups_obj);
    for (name, m) in &group_names {
        let val = if let Some(m) = m {
            js_nanbox_string(js_string_from_str(m.as_str()) as i64)
        } else {
            f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
        };
        let key_ptr = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
        let groups_obj = groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
        crate::object::js_object_set_field_by_name(groups_obj, key_ptr, val);
    }
    groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>()
}

/// Fancy-regex fallback for the string-replacement (non-callback) forms of
/// `String.prototype.replace`/`replaceAll`. Drives a manual non-overlapping
/// match loop with `fancy_regex` and expands the replacement string via
/// [`expand_js_replacement_fancy`]. Used when the pattern needs
/// lookbehind/backreferences the `regex` crate can't compile.
unsafe fn replace_regex_str_fancy(
    str_data: &str,
    fre: &fancy_regex::Regex,
    global: bool,
    repl_str: &str,
) -> *mut StringHeader {
    let has_named_groups = fre.capture_names().any(|n| n.is_some());
    let mut captures_list: Vec<fancy_regex::Captures> = Vec::new();
    let mut iter = fre.captures_iter(str_data);
    while let Some(Ok(caps)) = iter.next() {
        captures_list.push(caps);
        if !global {
            break;
        }
    }
    let mut result = String::new();
    let mut last_end = 0usize;
    for caps in &captures_list {
        let full_match = caps.get(0).unwrap();
        result.push_str(&str_data[last_end..full_match.start()]);
        result.push_str(&expand_js_replacement_fancy(
            repl_str,
            caps,
            str_data,
            has_named_groups,
        ));
        last_end = full_match.end();
    }
    result.push_str(&str_data[last_end..]);
    js_string_from_str(&result)
}

/// string.replace(regex, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let repl_str = if is_valid_ptr(replacement) {
        string_as_str(replacement)
    } else {
        "undefined"
    };

    if !is_valid_regex_ptr(re) {
        // If regex is null, return original string
        return js_string_from_str(str_data);
    }

    unsafe {
        // Pattern the `regex` crate couldn't compile (lookbehind/backreferences)
        // → drive the replacement through fancy-regex. Otherwise the never-match
        // placeholder in `regex_ptr` would leave the input unchanged.
        if let Some(fre) = lookup_fancy_regex(re) {
            return replace_regex_str_fancy(str_data, &fre, (*re).global, repl_str);
        }

        let regex = &*(*re).regex_ptr;
        let global = (*re).global;
        let has_named_groups = regex.capture_names().any(|n| n.is_some());

        // Route through a JS-aware expander (closure form) so `$&` / `` $` `` /
        // `$'` — which the regex crate's native `$` syntax doesn't support —
        // are substituted per match. `$$`, `$n`, and `$<name>` are handled too.
        let result = if global {
            regex
                .replace_all(str_data, |caps: &regex::Captures| {
                    expand_js_replacement(repl_str, caps, str_data, has_named_groups)
                })
                .to_string()
        } else {
            regex
                .replace(str_data, |caps: &regex::Captures| {
                    expand_js_replacement(repl_str, caps, str_data, has_named_groups)
                })
                .to_string()
        };

        js_string_from_str(&result)
    }
}

/// string.replaceAll(regex, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    if !is_valid_regex_ptr(re) {
        return js_string_from_str(str_data);
    }

    ensure_replace_all_regex_global(re);
    js_string_replace_regex(s, re, replacement)
}

/// Split a string by a regex delimiter
/// string.split(regex) -> string[] (array of NaN-boxed string pointers)
#[no_mangle]
pub extern "C" fn js_string_split_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    js_string_split_regex_n(s, re, -1)
}

/// string.split(regex, limit) — limit<0 means no limit, limit==0 means empty
/// (issue #567).
#[no_mangle]
pub extern "C" fn js_string_split_regex_n(
    s: *const StringHeader,
    re: *const RegExpHeader,
    limit: i32,
) -> *mut ArrayHeader {
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    if !is_valid_ptr(s) {
        return crate::array::js_array_alloc(0);
    }
    if limit == 0 {
        return crate::array::js_array_alloc(0);
    }
    let str_data = string_as_str(s).to_owned();

    if !is_valid_regex_ptr(re) {
        // No regex: return array with the whole string as a single element
        let arr = crate::array::js_array_alloc(1);
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        let str_ptr = js_string_from_str(&str_data) as u64;
        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
        unsafe {
            (*arr).length = 1;
            let nanboxed = STRING_TAG | (str_ptr & POINTER_MASK);
            // GC_STORE_AUDIT(BARRIERED): regex split fallback slot uses the shared array slot-store helper.
            crate::array::store_array_slot(arr, 0, nanboxed);
        }
        return arr_handle.get_raw_mut_ptr::<ArrayHeader>();
    }

    unsafe {
        // Fancy-regex fallback (lookbehind/backreferences): `fancy_regex` has no
        // `split`, so walk non-overlapping matches and slice between them. This
        // mirrors the `regex` crate's `split` (delimiter text dropped, captured
        // groups NOT spliced into the result — same as the standard path here).
        let mut parts: Vec<&str> = if let Some(fre) = lookup_fancy_regex(re) {
            let mut v: Vec<&str> = Vec::new();
            let mut last = 0usize;
            let mut iter = fre.find_iter(&str_data);
            while let Some(Ok(m)) = iter.next() {
                v.push(&str_data[last..m.start()]);
                last = m.end();
            }
            v.push(&str_data[last..]);
            v
        } else {
            let regex = &*(*re).regex_ptr;
            regex.split(&str_data).collect()
        };
        if limit > 0 && (parts.len() as i64) > (limit as i64) {
            parts.truncate(limit as usize);
        }

        let arr = crate::array::js_array_alloc(parts.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = parts.len() as u32;

        for (i, part) in parts.iter().enumerate() {
            let str_ptr = js_string_from_str(part) as u64;
            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            let nanboxed = STRING_TAG | (str_ptr & POINTER_MASK);
            // GC_STORE_AUDIT(BARRIERED): regex split result slot uses the shared array slot-store helper.
            crate::array::store_array_slot(arr, i, nanboxed);
        }
        arr_handle.get_raw_mut_ptr::<ArrayHeader>()
    }
}

/// Search for a regex match in a string
/// string.search(regex) -> number (index of first match, -1 if none)
#[no_mangle]
pub extern "C" fn js_string_search_regex(s: *const StringHeader, re: *const RegExpHeader) -> i32 {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        return -1;
    }
    let str_data = string_as_str(s);

    unsafe {
        // Fancy-regex fallback (lookbehind/backreferences): the never-match
        // placeholder in `regex_ptr` would always report -1 otherwise.
        if let Some(fre) = lookup_fancy_regex(re) {
            return match fre.find(str_data) {
                Ok(Some(m)) => str_data[..m.start()].chars().count() as i32,
                _ => -1,
            };
        }

        let regex = &*(*re).regex_ptr;
        match regex.find(str_data) {
            Some(m) => {
                // Convert byte offset to char offset (JS indices are UTF-16 code units,
                // but for ASCII/BMP this matches char offset)
                let byte_offset = m.start();
                let char_offset = str_data[..byte_offset].chars().count();
                char_offset as i32
            }
            None => -1,
        }
    }
}

/// regex.exec(string) -> match array (like string.match) with thread-local index/groups
/// For global regexes, starts matching at lastIndex and updates it.
/// Returns *mut ArrayHeader (null for no match). Stores .index and .groups
/// in thread-locals, retrieved via js_regexp_exec_get_index / js_regexp_exec_get_groups.
#[no_mangle]
pub extern "C" fn js_regexp_exec(
    re: *mut RegExpHeader,
    s: *const StringHeader,
) -> *mut crate::array::ArrayHeader {
    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    // #854: POINTER_TAG / POINTER_MASK kept co-located with the NaN-box
    // tag contract even when this exec helper only reads TAG_UNDEFINED.
    // Codegen and sibling helpers in regex.rs use the same values.
    #[allow(dead_code)]
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
    #[allow(dead_code)]
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
        return ptr::null_mut();
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;
        let sticky = (*re).sticky;
        // Per spec RegExpBuiltinExec, `lastIndex` drives the search start for
        // BOTH global and sticky regexes (and lastIndex is reset/updated for
        // either). A sticky match must additionally *anchor* at lastIndex.
        let use_last_index = global || sticky;
        // Spec: for non-global/non-sticky, lastIndex is treated as 0 and NOT
        // read (so a `valueOf`-bearing lastIndex isn't observed). Only consult
        // (and ToLength-coerce) it when stateful.
        let last_index = if use_last_index {
            regex_last_index_offset(re)
        } else {
            0
        };

        let search_start_byte = if use_last_index && last_index > 0 {
            let mut byte_off = 0;
            let mut char_count = 0;
            for ch in str_data.chars() {
                if char_count >= last_index {
                    break;
                }
                byte_off += ch.len_utf8();
                char_count += 1;
            }
            byte_off
        } else {
            0
        };

        if search_start_byte > str_data.len() {
            if use_last_index {
                store_last_index_number(re, 0);
            }
            LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
            LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
            return ptr::null_mut();
        }

        let search_str = &str_data[search_start_byte..];

        // Check if this regex has a fancy-regex fallback (lookbehind/lookahead).
        let fancy_captures = FANCY_CACHE.with(|fc| {
            let fc = fc.borrow();
            let pat = string_as_str((*re).pattern_ptr);
            let flags_str = string_as_str((*re).flags_ptr);
            if let Some(fre) = fc.get(&(pat.to_string(), flags_str.to_string())) {
                if let Ok(Some(caps)) = fre.captures(search_str) {
                    let full = caps.get(0).unwrap();
                    // Sticky (`y`) requires the match to start exactly at
                    // lastIndex — i.e. offset 0 of the sliced search string.
                    if sticky && full.start() != 0 {
                        return Some(ptr::null_mut());
                    }
                    let match_byte_offset = full.start() + search_start_byte;
                    let match_char_offset = str_data[..match_byte_offset].chars().count();
                    let arr = crate::array::js_array_alloc(caps.len() as u32);
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let arr_handle = scope.root_raw_mut_ptr(arr);
                    (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;
                    for i in 0..caps.len() {
                        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                        if let Some(m) = caps.get(i) {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            // GC_STORE_AUDIT(BARRIERED): regex exec fancy capture slot uses the shared array slot-store helper.
                            crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                        } else {
                            let undefined = f64::from_bits(TAG_UNDEFINED);
                            // GC_STORE_AUDIT(BARRIERED): regex exec fancy unmatched capture slot uses the shared array slot-store helper.
                            crate::array::store_array_slot(arr, i, undefined.to_bits());
                        }
                    }
                    if use_last_index {
                        let match_str = full.as_str();
                        store_last_index_number(re, match_char_offset + match_str.chars().count());
                    }
                    set_exec_array_metadata(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        str_data,
                        match_char_offset as f64,
                    );
                    LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = match_char_offset as f64);
                    // Extract named-capture groups through the fancy path so
                    // `/(?<=x)(?<y>\d+)/.exec(s).groups` works for patterns the
                    // `regex` crate can't compile.
                    let groups_obj = build_fancy_groups(fre, &caps, &scope);
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups_obj);
                    set_exec_array_groups(arr_handle.get_raw_mut_ptr::<ArrayHeader>(), groups_obj);
                    return Some(arr_handle.get_raw_mut_ptr::<ArrayHeader>());
                }
                return Some(ptr::null_mut()); // fancy-regex tried but no match
            }
            None // no fancy fallback — use standard regex
        });
        if let Some(result) = fancy_captures {
            if result.is_null() {
                if use_last_index {
                    store_last_index_number(re, 0);
                }
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                return ptr::null_mut();
            }
            return result;
        }

        let standard_caps = regex.captures(search_str).filter(|caps| {
            // Sticky (`y`) requires the match to start at lastIndex (offset 0 of
            // the slice); a leftmost match further in does not count.
            !sticky || caps.get(0).map(|m| m.start() == 0).unwrap_or(false)
        });
        match standard_caps {
            Some(caps) => {
                let match_byte_offset = caps.get(0).unwrap().start() + search_start_byte;
                let match_char_offset = str_data[..match_byte_offset].chars().count();

                if use_last_index {
                    let match_end_byte = caps.get(0).unwrap().end() + search_start_byte;
                    let match_end_char = str_data[..match_end_byte].chars().count();
                    store_last_index_number(re, match_end_char);
                }

                // Create match array: [fullMatch, group1, group2, ...]
                let arr = crate::array::js_array_alloc(caps.len() as u32);
                let scope = crate::gc::RuntimeHandleScope::new();
                let arr_handle = scope.root_raw_mut_ptr(arr);
                (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

                for (i, cap) in caps.iter().enumerate() {
                    if let Some(m) = cap {
                        let str_ptr = js_string_from_str(m.as_str());
                        let nanboxed = js_nanbox_string(str_ptr as i64);
                        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                        // GC_STORE_AUDIT(BARRIERED): regex exec capture slot uses the shared array slot-store helper.
                        crate::array::store_array_slot(arr, i, nanboxed.to_bits());
                    } else {
                        let undefined = f64::from_bits(TAG_UNDEFINED);
                        let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                        // GC_STORE_AUDIT(BARRIERED): regex exec unmatched capture slot uses the shared array slot-store helper.
                        crate::array::store_array_slot(arr, i, undefined.to_bits());
                    }
                }

                // Store .index in thread-local
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = match_char_offset as f64);
                set_exec_array_metadata(
                    arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                    str_data,
                    match_char_offset as f64,
                );

                // Build groups object if named captures exist
                let group_names: Vec<(&str, Option<regex::Match>)> = regex
                    .capture_names()
                    .enumerate()
                    .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
                    .collect();

                if !group_names.is_empty() {
                    // Allocate a fresh per-result object (and shape) via
                    // `js_object_alloc(0, 0)` + by-name setters, NOT a shared
                    // `js_object_alloc_with_shape(const_id)`. A fixed interned
                    // shape id makes a later match with different named captures
                    // inherit the prior call's key names (e.g. `(?<x>…)` then
                    // `(?<z>…)` exposing `.x` on the second result). This mirrors
                    // the fix already applied to the `js_string_match` path.
                    let groups_obj = crate::object::js_object_alloc(0, 0);
                    let groups_handle = scope.root_raw_mut_ptr(groups_obj);
                    for (name, m) in &group_names {
                        let val = if let Some(m) = m {
                            let str_ptr = js_string_from_str(m.as_str());
                            js_nanbox_string(str_ptr as i64)
                        } else {
                            f64::from_bits(TAG_UNDEFINED)
                        };
                        let key_ptr =
                            crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
                        let groups_obj =
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                        crate::object::js_object_set_field_by_name(groups_obj, key_ptr, val);
                    }
                    LAST_EXEC_GROUPS.with(|g| {
                        *g.borrow_mut() =
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>()
                    });
                    set_exec_array_groups(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
                    );
                } else {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                    set_exec_array_groups(
                        arr_handle.get_raw_mut_ptr::<ArrayHeader>(),
                        ptr::null_mut(),
                    );
                }

                arr_handle.get_raw_mut_ptr::<ArrayHeader>()
            }
            None => {
                if use_last_index {
                    store_last_index_number(re, 0);
                }
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                ptr::null_mut()
            }
        }
    }
}

/// Dynamic-receiver dispatch for `regex.test(str)` / `regex.exec(str)` when
/// codegen couldn't prove the receiver is a RegExp (e.g. hono's RegExpRouter
/// does `buildWildcardRegExp(k).test(path)`, where the receiver is the result
/// of a function call). Returns `Some(result)` only when `ptr` is a live regex
/// AND `method` is `test`/`exec`; `None` otherwise so the generic method
/// dispatch in `js_native_call_method` continues. The argument is coerced to a
/// string (`re.test(123)` tests against `"123"`). (#1731)
pub(crate) fn dispatch_regex_receiver_method(
    ptr: *const u8,
    method: &str,
    arg0: f64,
) -> Option<f64> {
    if !is_regex_pointer(ptr) {
        return None;
    }
    let re = ptr as *mut RegExpHeader;
    let s_ptr = crate::value::js_jsvalue_to_string(arg0);
    match method {
        "test" => {
            let matched = js_regexp_test(re, s_ptr) != 0;
            Some(f64::from_bits(crate::value::JSValue::bool(matched).bits()))
        }
        // exec: the match array, or `null` on no match (spec-correct).
        "exec" => {
            let arr = js_regexp_exec(re, s_ptr);
            Some(if arr.is_null() {
                f64::from_bits(crate::value::TAG_NULL)
            } else {
                f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
            })
        }
        // `regex.toString()` → `/source/flags` (RegExp.prototype.toString).
        "toString" => {
            let s = js_regexp_to_string(re);
            Some(f64::from_bits(
                crate::value::js_nanbox_string(s as i64).to_bits(),
            ))
        }
        _ => None,
    }
}

/// Get the .index from the last exec() call
#[no_mangle]
pub extern "C" fn js_regexp_exec_get_index() -> f64 {
    LAST_EXEC_INDEX.with(|idx| *idx.borrow())
}

/// Get the .groups object from the last exec() call
/// Returns I64 pointer (0 for no groups)
#[no_mangle]
pub extern "C" fn js_regexp_exec_get_groups() -> i64 {
    LAST_EXEC_GROUPS.with(|g| {
        let ptr = *g.borrow();
        if ptr.is_null() {
            0
        } else {
            ptr as i64
        }
    })
}

/// GC root scanner for `LAST_EXEC_GROUPS`. The groups object built by
/// `js_regexp_exec` / `js_string_match` is stashed in this thread-local
/// for later `m.groups` reads — without scanning it as a root, a GC
/// firing between the match call and the property read can reclaim the
/// object, and subsequent reads dereference freed memory. Surfaced when
/// the `m.groups` fold was extended to cover `str.match(regex)` results
/// alongside `regex.exec(str)`: a sequence of match calls plus
/// allocations between them was enough to trigger nursery GC mid-test.
pub fn scan_last_exec_groups_root(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_last_exec_groups_root_mut(&mut visitor);
}

pub fn scan_last_exec_groups_root_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    LAST_EXEC_GROUPS.with(|g| {
        visitor.visit_raw_mut_ptr_slot(&mut *g.borrow_mut());
    });
}

#[cfg(test)]
pub(crate) fn test_set_last_exec_groups(ptr: *mut ObjectHeader) {
    LAST_EXEC_GROUPS.with(|g| {
        *g.borrow_mut() = ptr;
    });
}

#[cfg(test)]
pub(crate) fn test_last_exec_groups() -> usize {
    LAST_EXEC_GROUPS.with(|g| *g.borrow() as usize)
}

/// Get regex.source — returns the pattern string
#[no_mangle]
pub extern "C" fn js_regexp_get_source(re: *const RegExpHeader) -> *mut StringHeader {
    if !is_valid_regex_ptr(re) {
        return js_string_from_str("(?:)");
    }
    // Issue #637: prefer the side-tabled owned copy so we survive GC
    // of the input StringHeader (e.g. template-literal temporary).
    if let Some(pat) =
        REGEX_SOURCE_TABLE.with(|t| t.borrow().get(&(re as usize)).map(|(p, _)| p.clone()))
    {
        return js_string_from_str(&escape_regexp_source(&pat));
    }
    unsafe {
        if is_valid_ptr((*re).pattern_ptr) {
            // Return a copy of the pattern string
            let pattern_str = string_as_str((*re).pattern_ptr);
            js_string_from_str(&escape_regexp_source(pattern_str))
        } else {
            js_string_from_str("(?:)")
        }
    }
}

/// `RegExp.prototype.source` for the prototype object itself (no
/// `[[OriginalSource]]`) returns the canonical empty source `"(?:)"`.
#[no_mangle]
pub extern "C" fn js_regexp_empty_source() -> *mut StringHeader {
    js_string_from_str("(?:)")
}

/// ECMA-262 22.2.6.10 EscapeRegExpPattern: produce a string that, placed
/// between two `/` characters, parses as the same pattern. An empty pattern
/// becomes `"(?:)"`; an unescaped `/` outside a character class becomes `\/`;
/// the four LineTerminators become their `\n`/`\r`/` `/` ` escapes
/// (even inside a character class). A backslash escapes the following code
/// point, which is copied verbatim.
fn escape_regexp_source(pattern: &str) -> String {
    if pattern.is_empty() {
        return "(?:)".to_string();
    }
    let mut out = String::with_capacity(pattern.len() + 2);
    let mut in_class = false;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push('\\');
                if let Some(&next) = chars.peek() {
                    out.push(next);
                    chars.next();
                }
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
            }
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '/' if !in_class => out.push_str("\\/"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(c),
        }
    }
    out
}

/// Get regex.flags — returns the flags string
#[no_mangle]
pub extern "C" fn js_regexp_get_flags(re: *const RegExpHeader) -> *mut StringHeader {
    if !is_valid_regex_ptr(re) {
        return js_string_from_str("");
    }
    // Issue #637: prefer the side-tabled owned copy.
    if let Some(flags) =
        REGEX_SOURCE_TABLE.with(|t| t.borrow().get(&(re as usize)).map(|(_, f)| f.clone()))
    {
        return js_string_from_str(&flags);
    }
    unsafe {
        if is_valid_ptr((*re).flags_ptr) {
            let flags_str = string_as_str((*re).flags_ptr);
            js_string_from_str(flags_str)
        } else {
            js_string_from_str("")
        }
    }
}

/// `RegExp.prototype.toString()` — `/source/flags`. Used by both the
/// `regex.toString()` method dispatch and ToString coercion (`String(re)`,
/// template literals). Node never produces `"[object Object]"` for a RegExp.
#[no_mangle]
pub extern "C" fn js_regexp_to_string(re: *const RegExpHeader) -> *mut StringHeader {
    let src = js_regexp_get_source(re);
    let flg = js_regexp_get_flags(re);
    let out = unsafe { format!("/{}/{}", string_as_str(src), string_as_str(flg)) };
    js_string_from_str(&out)
}

/// Get regex.lastIndex — returns the stored value (NaN-boxed JSValue bits as
/// f64). Usually a number, but `re.lastIndex = obj` round-trips the object.
#[no_mangle]
pub extern "C" fn js_regexp_get_last_index(re: *const RegExpHeader) -> f64 {
    if !is_valid_regex_ptr(re) {
        return 0.0;
    }
    unsafe { f64::from_bits((*re).last_index) }
}

/// Set regex.lastIndex — stores the value verbatim (no coercion on write, per
/// spec `Set(R, "lastIndex", v)`).
#[no_mangle]
pub extern "C" fn js_regexp_set_last_index(re: *mut RegExpHeader, value: f64) {
    if !is_valid_regex_ptr(re) {
        return;
    }
    unsafe {
        (*re).last_index = value.to_bits();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::string::js_string_from_bytes;

    fn make_string(s: &str) -> *mut StringHeader {
        js_string_from_bytes(s.as_ptr(), s.len() as u32)
    }

    #[test]
    fn js_replacement_expands_special_patterns() {
        let re = regex::Regex::new(r"(\w+)\s(\w+)").unwrap();
        let subj = "John Smith";
        let caps = re.captures(subj).unwrap();
        assert_eq!(
            expand_js_replacement("$2 $1", &caps, subj, false),
            "Smith John"
        );
        assert_eq!(
            expand_js_replacement("[$&]", &caps, subj, false),
            "[John Smith]"
        );

        // $` (before) / $' (after) with a mid-string single-char match.
        let re2 = regex::Regex::new("b").unwrap();
        let s2 = "abc";
        let c2 = re2.captures(s2).unwrap();
        assert_eq!(expand_js_replacement("$`", &c2, s2, false), "a");
        assert_eq!(expand_js_replacement("$'", &c2, s2, false), "c");
        assert_eq!(expand_js_replacement("$&", &c2, s2, false), "b");
        assert_eq!(expand_js_replacement("$$", &c2, s2, false), "$"); // escaped literal
        assert_eq!(expand_js_replacement("$z", &c2, s2, false), "$z"); // invalid → literal
        assert_eq!(expand_js_replacement("end$", &c2, s2, false), "end$"); // trailing $

        // Numbered groups: two-digit-then-one-digit fallback + unmatched → "".
        let re3 = regex::Regex::new(r"(a)(x)?(b)").unwrap();
        let s3 = "ab";
        let c3 = re3.captures(s3).unwrap();
        assert_eq!(expand_js_replacement("$1$2$3", &c3, s3, false), "ab"); // $2 unmatched → ""
        assert_eq!(expand_js_replacement("$10", &c3, s3, false), "a0"); // no group 10 → $1 then '0'
    }

    #[test]
    fn js_replacement_named_group_gate() {
        // No named groups in the regex → `$<name>` is emitted literally (#2421).
        let re = regex::Regex::new("n").unwrap();
        let subj = "end";
        let caps = re.captures(subj).unwrap();
        assert_eq!(
            expand_js_replacement("$<bad>", &caps, subj, false),
            "$<bad>"
        );
        assert_eq!(
            expand_js_replacement("[$<bad>]", &caps, subj, false),
            "[$<bad>]"
        );

        // Named groups present: known name substitutes, unknown name → "".
        let re2 = regex::Regex::new(r"(?<first>\w+)\s(?<last>\w+)").unwrap();
        let subj2 = "John Smith";
        let caps2 = re2.captures(subj2).unwrap();
        assert_eq!(
            expand_js_replacement("$<last>, $<first>", &caps2, subj2, true),
            "Smith, John"
        );
        assert_eq!(
            expand_js_replacement("[$<missing>]", &caps2, subj2, true),
            "[]"
        );
    }

    // ---- #4797: fancy-regex fallback wired through every operation ----

    #[test]
    fn fancy_backreference_match() {
        // `(\w)\1` needs backreferences → fancy-regex fallback.
        let re = js_regexp_new(make_string(r"(\w)\1"), make_string(""));
        let result = js_string_match(make_string("hello"), re);
        assert!(!result.is_null());
        unsafe {
            let v = crate::array::js_array_get_f64(result, 0);
            let sp = crate::value::js_get_string_pointer_unified(v) as *const StringHeader;
            assert_eq!(string_as_str(sp), "ll");
        }
    }

    #[test]
    fn fancy_lookbehind_search() {
        let re = js_regexp_new(make_string(r"(?<==)\w+"), make_string(""));
        assert_eq!(js_string_search_regex(make_string("foo=bar"), re), 4);
        // No match → -1.
        let re2 = js_regexp_new(make_string(r"(?<==)\w+"), make_string(""));
        assert_eq!(js_string_search_regex(make_string("nomatch"), re2), -1);
    }

    #[test]
    fn fancy_lookbehind_split() {
        // Zero-width lookbehind split: "a1b2c3" → ["a1","b2","c3",""].
        let re = js_regexp_new(make_string(r"(?<=\d)"), make_string(""));
        let arr = js_string_split_regex(make_string("a1b2c3"), re);
        unsafe {
            assert_eq!((*arr).length, 4);
            let first = crate::array::js_array_get_f64(arr, 0);
            let sp = crate::value::js_get_string_pointer_unified(first) as *const StringHeader;
            assert_eq!(string_as_str(sp), "a1");
        }
    }

    #[test]
    fn fancy_lookbehind_replace_string() {
        // `$&` substitution under a lookbehind pattern the regex crate rejects.
        let re = js_regexp_new(make_string(r"(?<=\$)\d+"), make_string("g"));
        let out = js_string_replace_regex(make_string("$5 and $10"), re, make_string("[$&]"));
        assert_eq!(string_as_str(out), "$[5] and $[10]");
    }

    #[test]
    fn fancy_named_group_replace() {
        // `$<n>` named-group substitution through the fancy fallback.
        let re = js_regexp_new(make_string(r"(?<=\$)(?<n>\d+)"), make_string("g"));
        let out =
            js_string_replace_regex_named(make_string("$5 and $10"), re, make_string("[$<n>]"));
        assert_eq!(string_as_str(out), "$[5] and $[10]");
    }

    #[test]
    fn fancy_lookbehind_exec_index() {
        // exec() through the fancy path reports the char index of the match.
        let re = js_regexp_new(make_string(r"(?<=\$)\d+"), make_string(""));
        let result = js_regexp_exec(re, make_string("price: $42"));
        assert!(!result.is_null());
        assert_eq!(js_regexp_exec_get_index(), 8.0);
        unsafe {
            let v = crate::array::js_array_get_f64(result, 0);
            let sp = crate::value::js_get_string_pointer_unified(v) as *const StringHeader;
            assert_eq!(string_as_str(sp), "42");
        }
    }

    #[test]
    fn test_regexp_test_basic() {
        let pattern = make_string("hello");
        let flags = make_string("");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        assert!(js_regexp_test(re, test_str) != 0);

        let test_str2 = make_string("goodbye world");
        assert!(js_regexp_test(re, test_str2) == 0);
    }

    #[test]
    fn test_regexp_test_case_insensitive() {
        let pattern = make_string("hello");
        let flags = make_string("i");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("HELLO World");
        assert!(js_regexp_test(re, test_str) != 0);
    }

    #[test]
    fn test_string_match() {
        let pattern = make_string(r"\w+");
        let flags = make_string("");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let result = js_string_match(test_str, re);
        assert!(!result.is_null());

        unsafe {
            assert_eq!((*result).length, 1); // One match (first word)
        }
    }

    #[test]
    fn test_string_match_global() {
        let pattern = make_string(r"\w+");
        let flags = make_string("g");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let result = js_string_match(test_str, re);
        assert!(!result.is_null());

        unsafe {
            assert_eq!((*result).length, 2); // Two matches (hello, world)
        }
    }

    #[test]
    fn test_string_replace() {
        let pattern = make_string("world");
        let flags = make_string("");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let replacement = make_string("universe");
        let result = js_string_replace_regex(test_str, re, replacement);

        assert_eq!(string_as_str(result), "hello universe");
    }

    #[test]
    fn test_string_replace_global() {
        let pattern = make_string("o");
        let flags = make_string("g");
        let re = js_regexp_new(pattern, flags);

        let test_str = make_string("hello world");
        let replacement = make_string("0");
        let result = js_string_replace_regex(test_str, re, replacement);

        assert_eq!(string_as_str(result), "hell0 w0rld");
    }

    #[test]
    fn escaped_hyphen_in_class_stays_literal() {
        // #4425: `\-` inside a character class is always a literal hyphen. The
        // Rust `regex` crate reads a bare `-` flanked by members as a range
        // operator, so the escape must be preserved or `[a\- ]` translates to
        // the invalid range `[a- ]`.
        assert_eq!(js_regex_to_rust(r"[a\- ]"), r"[a\- ]");
        assert_eq!(js_regex_to_rust(r"[:\- ]"), r"[:\- ]");
        assert_eq!(js_regex_to_rust(r"[\-]"), r"[\-]");
        // Outside a class a hyphen carries no range meaning, so it stays bare.
        assert_eq!(js_regex_to_rust(r"a\-b"), "a-b");

        // The patterns that crashed `marked` at module-init must now compile.
        for pat in [r"[a\- ]", r"[:\- ]", r" {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n"] {
            let flags = make_string("");
            let re = js_regexp_new(make_string(pat), flags);
            assert!(!re.is_null(), "pattern failed to construct: {pat}");
        }
    }

    #[test]
    fn surrogate_pairs_fold_to_astral_scalars() {
        // High escape + low class → contiguous astral range.
        assert_eq!(
            js_regex_to_rust(r"\uD800[\uDC00-\uDC0B]"),
            r"[\x{10000}-\x{1000b}]"
        );
        // Two consecutive surrogate escapes → single astral scalar.
        assert_eq!(js_regex_to_rust(r"\uD83D\uDE00"), r"\x{1f600}");
        // High class + full low class → coalesced astral block.
        assert_eq!(
            js_regex_to_rust(r"[\uD80C\uD81C-\uD820][\uDC00-\uDFFF]"),
            r"[\x{13000}-\x{133ff}\x{17000}-\x{183ff}]"
        );
        // Non-surrogate escapes and ordinary classes are untouched.
        assert_eq!(js_regex_to_rust(r"[ˁ\xAA]"), r"[ˁ\xAA]");
        assert_eq!(js_regex_to_rust(r"[A-Za-z]"), r"[A-Za-z]");
        // A lone high surrogate (no following low unit) is left as-is.
        assert_eq!(js_regex_to_rust(r"\uD800x"), r"\uD800x");

        // The Test262 `nativeFunctionMatcher.js` ID regexes must now compile.
        let pat = r"(?:[A-Za-z\xAA]|\uD800[\uDC00-\uDC0B\uDC0D-\uDC26]|\uD801[\uDC00-\uDC9D])";
        let flags = make_string("");
        let re = js_regexp_new(make_string(pat), flags);
        assert!(!re.is_null(), "ID_Start-shaped pattern failed to construct");
    }
}
