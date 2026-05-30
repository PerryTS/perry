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
        let regex_pattern = if case_insensitive || multiline {
            let mut prefix = String::from("(?");
            if case_insensitive {
                prefix.push('i');
            }
            if multiline {
                prefix.push('m');
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
    /// lastIndex for global/sticky regexes (byte offset into the string for stateful exec)
    pub last_index: u32,
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

#[inline]
fn ensure_replace_all_regex_global(re: *const RegExpHeader) {
    unsafe {
        if !(*re).global {
            throw_replace_all_non_global_regex();
        }
    }
}

/// Translate a JavaScript regex pattern to a Rust regex-crate compatible pattern.
/// Handles JS-specific escape sequences not supported by the Rust regex crate.
/// Also converts JS-style named groups `(?<name>...)` to Rust-style `(?P<name>...)`.
fn js_regex_to_rust(pattern: &str) -> String {
    let mut result = String::with_capacity(pattern.len());
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                // JS allows \/ to escape forward slash — Rust regex doesn't need it
                '/' => {
                    result.push('/');
                    i += 2;
                }
                // Pass through all other backslash sequences as-is
                _ => {
                    result.push('\\');
                    result.push(chars[i + 1]);
                    i += 2;
                }
            }
        } else if chars[i] == '(' && i + 2 < chars.len() && chars[i + 1] == '?' {
            // Check for JS named group (?<name>...) — convert to (?P<name>...)
            // But NOT (?<=...) (lookbehind) or (?<!...) (negative lookbehind)
            if chars[i + 2] == '<'
                && i + 3 < chars.len()
                && chars[i + 3] != '='
                && chars[i + 3] != '!'
            {
                result.push_str("(?P<");
                i += 3; // skip past "(?<"
            } else {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
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
    let flags_str = if is_valid_ptr(flags) {
        string_as_str(flags)
    } else {
        ""
    };

    let case_insensitive = flags_str.contains('i');
    let global = flags_str.contains('g');
    let multiline = flags_str.contains('m');

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
    unsafe {
        let raw = crate::gc::gc_malloc(header_size, crate::gc::GC_TYPE_OBJECT);
        if raw.is_null() {
            panic!("Failed to allocate RegExp");
        }
        let ptr = raw as *mut RegExpHeader;

        (*ptr).regex_ptr = regex_ptr;
        (*ptr).pattern_ptr = pattern;
        (*ptr).flags_ptr = flags;
        (*ptr).case_insensitive = case_insensitive;
        (*ptr).global = global;
        (*ptr).multiline = multiline;
        (*ptr).last_index = 0;

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

/// Test if a string matches the regex pattern
/// regex.test(string) -> boolean
#[no_mangle]
pub extern "C" fn js_regexp_test(re: *const RegExpHeader, s: *const StringHeader) -> i32 {
    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        return 0;
    }

    let str_data = string_as_str(s);

    unsafe {
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
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
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
                    } else {
                        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
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

/// Find all matches in a string, each with capture groups
/// string.matchAll(regex) -> Array<Array<string>> (array of match arrays)
#[no_mangle]
pub extern "C" fn js_string_match_all(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    if !is_valid_ptr(s) || !is_valid_regex_ptr(re) {
        // Return empty array, not null (matchAll never returns null)
        return crate::array::js_array_alloc(0);
    }

    let str_data = string_as_str(s);

    unsafe {
        let regex = &*(*re).regex_ptr;

        // Collect all captures
        let all_caps: Vec<regex::Captures> = regex.captures_iter(str_data).collect();

        if all_caps.is_empty() {
            return crate::array::js_array_alloc(0);
        }

        // Create outer array (one entry per match)
        let outer = crate::array::js_array_alloc(all_caps.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let outer_handle = scope.root_raw_mut_ptr(outer);
        (*outer_handle.get_raw_mut_ptr::<ArrayHeader>()).length = all_caps.len() as u32;

        for (i, caps) in all_caps.iter().enumerate() {
            // Create inner array for this match (full match + capture groups)
            let inner = crate::array::js_array_alloc(caps.len() as u32);
            let inner_scope = crate::gc::RuntimeHandleScope::new();
            let inner_handle = inner_scope.root_raw_mut_ptr(inner);
            (*inner_handle.get_raw_mut_ptr::<ArrayHeader>()).length = caps.len() as u32;

            for (j, cap) in caps.iter().enumerate() {
                if let Some(m) = cap {
                    let str_ptr = js_string_from_str(m.as_str());
                    let nanboxed = js_nanbox_string(str_ptr as i64);
                    let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
                    // GC_STORE_AUDIT(BARRIERED): regex nested capture slot uses the shared array slot-store helper.
                    crate::array::store_array_slot(inner, j, nanboxed.to_bits());
                } else {
                    // Undefined capture group
                    let undefined = f64::from_bits(0x7FFC_0000_0000_0001);
                    let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
                    // GC_STORE_AUDIT(BARRIERED): regex unmatched nested capture slot uses the shared array slot-store helper.
                    crate::array::store_array_slot(inner, j, undefined.to_bits());
                }
            }

            // Store inner array as NaN-boxed POINTER_TAG in outer array slot —
            // raw `inner as i64 -> f64::from_bits` would write a non-NaN-boxed
            // double whose bits happen to alias the heap pointer; the codegen
            // IndexGet path then reads `arr[i]` as a plain number and crashes
            // when iterating with `for (const m of arr) m[1]`.
            let inner = inner_handle.get_raw_mut_ptr::<ArrayHeader>();
            let inner_boxed = crate::value::js_nanbox_pointer(inner as i64);
            let outer = outer_handle.get_raw_mut_ptr::<ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): regex nested result slot uses the shared array slot-store helper.
            crate::array::store_array_slot(outer, i, inner_boxed.to_bits());
        }

        outer_handle.get_raw_mut_ptr::<ArrayHeader>()
    }
}

/// Replace matches in a string
/// Expand a JS replacement string against one match, supporting the full set
/// of `String.prototype.replace` special patterns that the Rust `regex`
/// crate's own `$`-expansion does NOT cover: `$&` (matched substring),
/// `` $` `` (text before the match), `$'` (text after the match), plus the
/// shared `$$`, `$n`/`$nn` (numbered groups, largest-valid-group rule), and
/// `$<name>` (named groups). An unmatched group expands to the empty string;
/// an invalid `$`-sequence is emitted literally — both matching Node.
fn expand_js_replacement(
    repl: &str,
    caps: &regex::Captures,
    subject: &str,
    has_named_groups: bool,
) -> String {
    let m0 = match caps.get(0) {
        Some(m) => m,
        None => return String::new(),
    };
    let (mstart, mend) = (m0.start(), m0.end());
    let ngroups = caps.len(); // valid group indices are 1..ngroups
    let b = repl.as_bytes();
    let mut out = String::with_capacity(repl.len() + 16);
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            // Copy the run of non-`$` bytes in one go ('$' is ASCII, so the
            // slice boundaries are always on UTF-8 char boundaries).
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
                // JS tries the two-digit group first when it's valid, else
                // the single digit, else emits the `$` literally.
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
                // `$<name>` is a named-group reference ONLY when the regex
                // actually defines named capture groups. With no named groups,
                // JS emits `$<...>` literally (e.g. /n/ has none, so
                // "$<bad>" stays "$<bad>"). When the regex has named groups but
                // this particular name is absent, JS substitutes the empty
                // string.
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

/// Replace with a simple string pattern (not regex)
/// string.replace(pattern, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_string(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };
    let repl_str = if is_valid_ptr(replacement) {
        string_as_str(replacement)
    } else {
        "undefined"
    };

    // String.replace with a string pattern only replaces the first occurrence
    let result = str_data.replacen(pattern_str, repl_str, 1);
    js_string_from_str(&result)
}

/// Replace ALL occurrences with a simple string pattern (not regex)
/// string.replaceAll(pattern, replacement) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_all_string(
    s: *const StringHeader,
    pattern: *const StringHeader,
    replacement: *const StringHeader,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    let pattern_str = if is_valid_ptr(pattern) {
        string_as_str(pattern)
    } else {
        ""
    };
    let repl_str = if is_valid_ptr(replacement) {
        string_as_str(replacement)
    } else {
        "undefined"
    };

    let result = str_data.replace(pattern_str, repl_str);
    js_string_from_str(&result)
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
        let regex = &*(*re).regex_ptr;
        let mut parts: Vec<&str> = regex.split(&str_data).collect();
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
        let last_index = (*re).last_index as usize;

        let search_start_byte = if global && last_index > 0 {
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
            if global {
                (*re).last_index = 0;
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
                    // Build result: just the full match for now
                    let match_byte_offset = full.start() + search_start_byte;
                    let match_char_offset = str_data[..match_byte_offset].chars().count();
                    let match_str = full.as_str();
                    let arr = crate::array::js_array_alloc_with_length(1);
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let arr_handle = scope.root_raw_mut_ptr(arr);
                    let match_ptr = crate::string::js_string_from_bytes(
                        match_str.as_ptr(),
                        match_str.len() as u32,
                    );
                    let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
                    let match_bits =
                        crate::value::STRING_TAG | (match_ptr as u64 & crate::value::POINTER_MASK);
                    // GC_STORE_AUDIT(BARRIERED): regex exec fancy match slot uses the shared array slot-store helper.
                    crate::array::store_array_slot(arr, 0, match_bits);
                    if global {
                        (*re).last_index = (match_char_offset + match_str.chars().count()) as u32;
                    }
                    LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = match_char_offset as f64);
                    return Some(arr_handle.get_raw_mut_ptr::<ArrayHeader>());
                }
                return Some(ptr::null_mut()); // fancy-regex tried but no match
            }
            None // no fancy fallback — use standard regex
        });
        if let Some(result) = fancy_captures {
            if result.is_null() {
                if global {
                    (*re).last_index = 0;
                }
                LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
                LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                return ptr::null_mut();
            }
            return result;
        }

        match regex.captures(search_str) {
            Some(caps) => {
                let match_byte_offset = caps.get(0).unwrap().start() + search_start_byte;
                let match_char_offset = str_data[..match_byte_offset].chars().count();

                if global {
                    let match_end_byte = caps.get(0).unwrap().end() + search_start_byte;
                    let match_end_char = str_data[..match_end_byte].chars().count();
                    (*re).last_index = match_end_char as u32;
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

                // Build groups object if named captures exist
                let group_names: Vec<(&str, Option<regex::Match>)> = regex
                    .capture_names()
                    .enumerate()
                    .filter_map(|(i, name)| name.map(|n| (n, caps.get(i))))
                    .collect();

                if !group_names.is_empty() {
                    let mut packed_keys: Vec<u8> = Vec::new();
                    for (name, _) in &group_names {
                        packed_keys.extend_from_slice(name.as_bytes());
                        packed_keys.push(0);
                    }
                    let groups_obj = crate::object::js_object_alloc_with_shape(
                        0x7FFF_FE00,
                        group_names.len() as u32,
                        packed_keys.as_ptr(),
                        packed_keys.len() as u32,
                    );
                    let groups_handle = scope.root_raw_mut_ptr(groups_obj);
                    for (idx, (_, m)) in group_names.iter().enumerate() {
                        let val = if let Some(m) = m {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            crate::value::JSValue::from_bits(nanboxed.to_bits())
                        } else {
                            crate::value::JSValue::undefined()
                        };
                        let groups_obj =
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
                        crate::object::js_object_set_field(groups_obj, idx as u32, val);
                    }
                    LAST_EXEC_GROUPS.with(|g| {
                        *g.borrow_mut() =
                            groups_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>()
                    });
                } else {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                }

                arr_handle.get_raw_mut_ptr::<ArrayHeader>()
            }
            None => {
                if global {
                    (*re).last_index = 0;
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
        return js_string_from_str("");
    }
    // Issue #637: prefer the side-tabled owned copy so we survive GC
    // of the input StringHeader (e.g. template-literal temporary).
    if let Some(pat) =
        REGEX_SOURCE_TABLE.with(|t| t.borrow().get(&(re as usize)).map(|(p, _)| p.clone()))
    {
        return js_string_from_str(&pat);
    }
    unsafe {
        if is_valid_ptr((*re).pattern_ptr) {
            // Return a copy of the pattern string
            let pattern_str = string_as_str((*re).pattern_ptr);
            js_string_from_str(pattern_str)
        } else {
            js_string_from_str("")
        }
    }
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

/// Get regex.lastIndex — returns the current lastIndex value as f64
#[no_mangle]
pub extern "C" fn js_regexp_get_last_index(re: *const RegExpHeader) -> f64 {
    if !is_valid_regex_ptr(re) {
        return 0.0;
    }
    unsafe { (*re).last_index as f64 }
}

/// Set regex.lastIndex
#[no_mangle]
pub extern "C" fn js_regexp_set_last_index(re: *mut RegExpHeader, value: f64) {
    if !is_valid_regex_ptr(re) {
        return;
    }
    unsafe {
        (*re).last_index = value as u32;
    }
}

/// string.replace(regex, replacerFn) — replace with a callback function
/// The callback receives (match, p1, p2, ..., offset, string)
/// We simplify to (match, ...groups, offset) since the full string is rarely needed.
#[no_mangle]
pub extern "C" fn js_string_replace_regex_fn(
    s: *const StringHeader,
    re: *const RegExpHeader,
    callback: f64, // NaN-boxed closure pointer
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }
    let str_data = string_as_str(s);

    if !is_valid_regex_ptr(re) {
        return js_string_from_str(str_data);
    }

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;

        // Extract closure pointer from NaN-boxed value
        let closure_ptr =
            crate::value::js_nanbox_get_pointer(callback) as *const crate::closure::ClosureHeader;
        if closure_ptr.is_null() {
            return js_string_from_str(str_data);
        }

        let mut result = String::new();
        let mut last_end = 0usize;
        let captures_iter: Vec<regex::Captures> = if global {
            regex.captures_iter(str_data).collect()
        } else {
            match regex.captures(str_data) {
                Some(caps) => vec![caps],
                None => vec![],
            }
        };

        for caps in &captures_iter {
            let full_match = caps.get(0).unwrap();
            result.push_str(&str_data[last_end..full_match.start()]);

            // Calculate char offset for the offset parameter
            let char_offset = str_data[..full_match.start()].chars().count();

            // Call the closure with (match, ...groups, offset)
            // We need to use the appropriate js_closure_callN function
            let match_str = js_string_from_str(full_match.as_str());
            let match_nanboxed = js_nanbox_string(match_str as i64);

            let num_groups = caps.len() - 1; // exclude full match
            let ret = if num_groups == 0 {
                // Call with (match, offset)
                let offset_f64 = char_offset as f64;
                crate::closure::js_closure_call2(closure_ptr, match_nanboxed, offset_f64)
            } else if num_groups == 1 {
                // Call with (match, p1, offset)
                let p1 = if let Some(m) = caps.get(1) {
                    js_nanbox_string(js_string_from_str(m.as_str()) as i64)
                } else {
                    f64::from_bits(TAG_UNDEFINED)
                };
                let offset_f64 = char_offset as f64;
                crate::closure::js_closure_call3(closure_ptr, match_nanboxed, p1, offset_f64)
            } else {
                // For 2+ groups, call with (match, p1, p2, offset)
                let p1 = if let Some(m) = caps.get(1) {
                    js_nanbox_string(js_string_from_str(m.as_str()) as i64)
                } else {
                    f64::from_bits(TAG_UNDEFINED)
                };
                let p2 = if let Some(m) = caps.get(2) {
                    js_nanbox_string(js_string_from_str(m.as_str()) as i64)
                } else {
                    f64::from_bits(TAG_UNDEFINED)
                };
                let offset_f64 = char_offset as f64;
                crate::closure::js_closure_call4(closure_ptr, match_nanboxed, p1, p2, offset_f64)
            };

            // Convert the NaN-boxed return value to a string. Issue #833:
            // the previous tag-discriminated decode only handled STRING_TAG
            // (0x7FFF) and POINTER_TAG (0x7FFD) — it silently dropped
            // SHORT_STRING_TAG (0x7FF9) SSO values, so any replacer-fn
            // whose result fit in ≤5 bytes (`s.charAt(0) + s.slice(1)` on
            // a 5-byte input is exactly the edge case in the bug report)
            // produced an empty replacement. Route through
            // `js_get_string_pointer_unified` instead, which handles all
            // four string representations (heap STRING_TAG, SSO with
            // heap-materialization, POINTER_TAG, raw pointer) plus the
            // JS spec's number-to-string coercion for numeric returns.
            let ptr = crate::value::js_get_string_pointer_unified(ret) as *const StringHeader;
            if is_valid_ptr(ptr) {
                result.push_str(string_as_str(ptr));
            }

            last_end = full_match.end();
        }

        // Append remaining text
        result.push_str(&str_data[last_end..]);
        js_string_from_str(&result)
    }
}

/// string.replaceAll(regex, replacerFn) -> string
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex_fn(
    s: *const StringHeader,
    re: *const RegExpHeader,
    callback: f64,
) -> *mut StringHeader {
    if !is_valid_ptr(s) {
        return js_string_from_str("");
    }

    let str_data = string_as_str(s);
    if !is_valid_regex_ptr(re) {
        return js_string_from_str(str_data);
    }

    ensure_replace_all_regex_global(re);
    js_string_replace_regex_fn(s, re, callback)
}

/// string.replace(regex, replacement) with named group references ($<name>)
/// Handles $<name> replacement patterns for named capture groups
#[no_mangle]
pub extern "C" fn js_string_replace_regex_named(
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
        return js_string_from_str(str_data);
    }

    // Check if replacement contains $<name> patterns
    let has_named_refs = repl_str.contains("$<");

    if !has_named_refs {
        // Fall back to regular replace
        return js_string_replace_regex(s, re, replacement);
    }

    unsafe {
        let regex = &*(*re).regex_ptr;
        let global = (*re).global;
        let has_named_groups = regex.capture_names().any(|n| n.is_some());

        let mut result = String::new();
        let mut last_end = 0usize;

        let captures_list: Vec<regex::Captures> = if global {
            regex.captures_iter(str_data).collect()
        } else {
            match regex.captures(str_data) {
                Some(caps) => vec![caps],
                None => vec![],
            }
        };

        if captures_list.is_empty() {
            return js_string_from_str(str_data);
        }

        for caps in &captures_list {
            let full_match = caps.get(0).unwrap();
            result.push_str(&str_data[last_end..full_match.start()]);

            // Delegate to the unified JS-aware expander so `$<name>` follows the
            // spec: literal when the regex has no named groups, empty when the
            // named group is absent (and `$&`/`` $` ``/`$'`/`$n`/`$$` all work).
            result.push_str(&expand_js_replacement(
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
}

/// string.replaceAll(regex, replacement) with named group references ($<name>)
#[no_mangle]
pub extern "C" fn js_string_replace_all_regex_named(
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
    js_string_replace_regex_named(s, re, replacement)
}

// ============================================================================
// RegExp.escape (TC39 proposal, shipped in Node 24+) — issue #2899
// ============================================================================

/// ECMAScript `WhiteSpace` set: TAB/VT/FF/SP, NBSP, ZWNBSP, and all
/// Unicode `Space_Separator` (Zs) code points. (TAB/VT/FF are handled by
/// the named control-escape table first; included here for completeness.)
fn regexp_escape_is_whitespace(cp: u32) -> bool {
    matches!(
        cp,
        0x0009 // TAB
            | 0x000B // VT
            | 0x000C // FF
            | 0x0020 // SP
            | 0x00A0 // NBSP
            | 0xFEFF // ZWNBSP
            // Unicode Space_Separator (Zs):
            | 0x1680
            | 0x2000..=0x200A | 0x202F | 0x205F | 0x3000
    )
}

/// ECMAScript `LineTerminator` set: LF, CR, LS (U+2028), PS (U+2029).
fn regexp_escape_is_line_terminator(cp: u32) -> bool {
    matches!(cp, 0x000A | 0x000D | 0x2028 | 0x2029)
}

/// `EncodeForRegExpEscape` unicode-escape emitter: `\xHH` for ≤ 0xFF,
/// `\uHHHH` otherwise (callers only pass BMP code units here).
fn regexp_escape_unicode(out: &mut String, unit: u16) {
    if unit <= 0xFF {
        out.push_str(&format!("\\x{:02x}", unit));
    } else {
        out.push_str(&format!("\\u{:04x}", unit));
    }
}

/// `RegExp.escape(str)` — escape `str` so it can be embedded literally in a
/// regular expression pattern without changing match semantics. Operates on
/// UTF-16 code units to match JS string semantics. The argument MUST be a
/// string (TypeError otherwise). Returns a NaN-boxed string.
#[no_mangle]
pub extern "C" fn js_regexp_escape(input: f64) -> f64 {
    let jsv = crate::value::JSValue::from_bits(input.to_bits());
    if !jsv.is_any_string() {
        let msg = js_string_from_str("input argument must be a string");
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }

    let str_ptr = crate::value::js_get_string_pointer_unified(input) as *const StringHeader;
    let s = string_as_str(str_ptr);

    // Encode to UTF-16 code units: JS escaping is defined per code unit.
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = String::with_capacity(units.len() * 2);

    for (i, &unit) in units.iter().enumerate() {
        let c = char::from_u32(unit as u32);

        // First code unit: if ASCII alphanumeric, force a unicode escape so a
        // leading letter/digit can't combine with a preceding backslash when
        // concatenated (e.g. avoid forming `\c`, `\1`, etc.).
        if i == 0 {
            if let Some(ch) = c {
                if ch.is_ascii_alphanumeric() {
                    regexp_escape_unicode(&mut out, unit);
                    continue;
                }
            }
        }

        match c {
            // Syntax characters and `/` → backslash escape.
            Some('^') | Some('$') | Some('\\') | Some('.') | Some('*') | Some('+') | Some('?')
            | Some('(') | Some(')') | Some('[') | Some(']') | Some('{') | Some('}') | Some('|')
            | Some('/') => {
                out.push('\\');
                out.push(c.unwrap());
            }
            // Named control escapes.
            Some('\t') => out.push_str("\\t"),
            Some('\n') => out.push_str("\\n"),
            Some('\u{000B}') => out.push_str("\\v"),
            Some('\u{000C}') => out.push_str("\\f"),
            Some('\r') => out.push_str("\\r"),
            _ => {
                let cp = unit as u32;
                let is_other_punctuator = matches!(
                    c,
                    Some(',')
                        | Some('-')
                        | Some('=')
                        | Some('<')
                        | Some('>')
                        | Some('#')
                        | Some('&')
                        | Some('!')
                        | Some('%')
                        | Some(':')
                        | Some(';')
                        | Some('@')
                        | Some('~')
                        | Some('\'')
                        | Some('`')
                        | Some('"')
                );
                if is_other_punctuator
                    || regexp_escape_is_whitespace(cp)
                    || regexp_escape_is_line_terminator(cp)
                {
                    regexp_escape_unicode(&mut out, unit);
                } else {
                    // Pass through. Use the original code unit so lone
                    // surrogates round-trip (char::from_u32 returns None for
                    // surrogate halves; push the decoded char when valid).
                    match c {
                        Some(ch) => out.push(ch),
                        None => {
                            // Lone surrogate: re-encode the single code unit.
                            let mut buf = [0u16; 1];
                            buf[0] = unit;
                            out.push_str(&String::from_utf16_lossy(&buf));
                        }
                    }
                }
            }
        }
    }

    let result = js_string_from_str(&out);
    js_nanbox_string(result as i64)
}

/// Keepalive anchor: `js_regexp_escape` is only called from codegen-emitted
/// `.o`, so the auto-optimize whole-program LLVM rebuild would dead-strip it
/// without this `#[used]` reference (see #3320).
#[used]
static KEEP_REGEXP_ESCAPE: extern "C" fn(f64) -> f64 = js_regexp_escape;

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
}
