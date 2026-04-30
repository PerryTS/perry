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
        let regex = &*(*re).regex_ptr;
        if regex.is_match(str_data) {
            1
        } else {
            0
        }
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

        if global {
            // Global flag: return all matches
            let matches: Vec<&str> = regex.find_iter(str_data).map(|m| m.as_str()).collect();

            if matches.is_empty() {
                return ptr::null_mut();
            }

            // Create array of string pointers
            let arr = crate::array::js_array_alloc(matches.len() as u32);
            (*arr).length = matches.len() as u32;
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

            for (i, m) in matches.iter().enumerate() {
                let str_ptr = js_string_from_str(m);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                std::ptr::write(elements_ptr.add(i), nanboxed);
            }

            arr
        } else {
            // Non-global: return first match only (or with capture groups)
            match regex.captures(str_data) {
                Some(caps) => {
                    // Return array with full match and capture groups
                    let arr = crate::array::js_array_alloc(caps.len() as u32);
                    (*arr).length = caps.len() as u32;
                    let elements_ptr =
                        (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

                    for (i, cap) in caps.iter().enumerate() {
                        if let Some(m) = cap {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            std::ptr::write(elements_ptr.add(i), nanboxed);
                        } else {
                            // Undefined capture group - store as undefined (TAG_UNDEFINED = 0x7FFC_0000_0000_0001)
                            std::ptr::write(
                                elements_ptr.add(i),
                                f64::from_bits(0x7FFC_0000_0000_0001),
                            );
                        }
                    }

                    arr
                }
                None => ptr::null_mut(),
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
        (*outer).length = all_caps.len() as u32;
        let outer_elements = (outer as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for (i, caps) in all_caps.iter().enumerate() {
            // Create inner array for this match (full match + capture groups)
            let inner = crate::array::js_array_alloc(caps.len() as u32);
            (*inner).length = caps.len() as u32;
            let inner_elements =
                (inner as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

            for (j, cap) in caps.iter().enumerate() {
                if let Some(m) = cap {
                    let str_ptr = js_string_from_str(m.as_str());
                    let nanboxed = js_nanbox_string(str_ptr as i64);
                    std::ptr::write(inner_elements.add(j), nanboxed);
                } else {
                    // Undefined capture group
                    std::ptr::write(inner_elements.add(j), f64::from_bits(0x7FFC_0000_0000_0001));
                }
            }

            // Store inner array as NaN-boxed POINTER_TAG in outer array slot —
            // raw `inner as i64 -> f64::from_bits` would write a non-NaN-boxed
            // double whose bits happen to alias the heap pointer; the codegen
            // IndexGet path then reads `arr[i]` as a plain number and crashes
            // when iterating with `for (const m of arr) m[1]`.
            std::ptr::write(
                outer_elements.add(i),
                crate::value::js_nanbox_pointer(inner as i64),
            );
        }

        outer
    }
}

/// Replace matches in a string
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

        let result = if global {
            // Global flag: replace all occurrences
            regex.replace_all(str_data, repl_str).to_string()
        } else {
            // Non-global: replace first occurrence only
            regex.replace(str_data, repl_str).to_string()
        };

        js_string_from_str(&result)
    }
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
    const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
    const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

    if !is_valid_ptr(s) {
        return crate::array::js_array_alloc(0);
    }
    let str_data = string_as_str(s);

    if !is_valid_regex_ptr(re) {
        // No regex: return array with the whole string as a single element
        let arr = crate::array::js_array_alloc(1);
        unsafe {
            (*arr).length = 1;
            let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;
            let str_ptr = js_string_from_str(str_data) as u64;
            let nanboxed = STRING_TAG | (str_ptr & POINTER_MASK);
            std::ptr::write(elements_ptr, f64::from_bits(nanboxed));
        }
        return arr;
    }

    unsafe {
        let regex = &*(*re).regex_ptr;
        let parts: Vec<&str> = regex.split(str_data).collect();

        let arr = crate::array::js_array_alloc(parts.len() as u32);
        (*arr).length = parts.len() as u32;
        let elements_ptr = (arr as *mut u8).add(std::mem::size_of::<ArrayHeader>()) as *mut f64;

        for (i, part) in parts.iter().enumerate() {
            let str_ptr = js_string_from_str(part) as u64;
            let nanboxed = STRING_TAG | (str_ptr & POINTER_MASK);
            std::ptr::write(elements_ptr.add(i), f64::from_bits(nanboxed));
        }
        arr
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
    const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
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
                    let match_ptr = crate::string::js_string_from_bytes(
                        match_str.as_ptr(),
                        match_str.len() as u32,
                    );
                    let arr = crate::array::js_array_alloc_with_length(1);
                    let elements = (arr as *mut u8).add(8) as *mut f64;
                    *elements = f64::from_bits(
                        crate::value::STRING_TAG | (match_ptr as u64 & crate::value::POINTER_MASK),
                    );
                    if global {
                        (*re).last_index = (match_char_offset + match_str.chars().count()) as u32;
                    }
                    LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = match_char_offset as f64);
                    return Some(arr);
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
                (*arr).length = caps.len() as u32;
                let elements_ptr = (arr as *mut u8)
                    .add(std::mem::size_of::<crate::array::ArrayHeader>())
                    as *mut f64;

                for (i, cap) in caps.iter().enumerate() {
                    if let Some(m) = cap {
                        let str_ptr = js_string_from_str(m.as_str());
                        let nanboxed = js_nanbox_string(str_ptr as i64);
                        std::ptr::write(elements_ptr.add(i), nanboxed);
                    } else {
                        std::ptr::write(elements_ptr.add(i), f64::from_bits(TAG_UNDEFINED));
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
                    for (idx, (_, m)) in group_names.iter().enumerate() {
                        let val = if let Some(m) = m {
                            let str_ptr = js_string_from_str(m.as_str());
                            let nanboxed = js_nanbox_string(str_ptr as i64);
                            crate::value::JSValue::from_bits(nanboxed.to_bits())
                        } else {
                            crate::value::JSValue::undefined()
                        };
                        crate::object::js_object_set_field(groups_obj, idx as u32, val);
                    }
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = groups_obj);
                } else {
                    LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
                }

                arr
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

/// Get regex.source — returns the pattern string
#[no_mangle]
pub extern "C" fn js_regexp_get_source(re: *const RegExpHeader) -> *mut StringHeader {
    if !is_valid_regex_ptr(re) {
        return js_string_from_str("");
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

            // Convert the NaN-boxed return value to a string
            // The callback should return a string (NaN-boxed with STRING_TAG = 0x7FFF)
            let bits = ret.to_bits();
            let tag = (bits >> 48) & 0xFFFF;
            if tag == 0x7FFF {
                // STRING_TAG: extract pointer from lower 48 bits
                let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
                if is_valid_ptr(ptr) {
                    result.push_str(string_as_str(ptr));
                }
            } else if tag == 0x7FFD {
                // POINTER_TAG: might be a string pointer that was NaN-boxed differently
                let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const StringHeader;
                if is_valid_ptr(ptr) {
                    result.push_str(string_as_str(ptr));
                }
            }

            last_end = full_match.end();
        }

        // Append remaining text
        result.push_str(&str_data[last_end..]);
        js_string_from_str(&result)
    }
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

            // Process the replacement string, substituting $<name> references
            let mut repl_result = String::new();
            let repl_chars: Vec<char> = repl_str.chars().collect();
            let mut ri = 0;
            while ri < repl_chars.len() {
                if repl_chars[ri] == '$' && ri + 1 < repl_chars.len() {
                    if repl_chars[ri + 1] == '<' {
                        // Named group reference: $<name>
                        let name_start = ri + 2;
                        if let Some(name_end) =
                            repl_chars[name_start..].iter().position(|&c| c == '>')
                        {
                            let name: String = repl_chars[name_start..name_start + name_end]
                                .iter()
                                .collect();
                            if let Some(m) = caps.name(&name) {
                                repl_result.push_str(m.as_str());
                            }
                            ri = name_start + name_end + 1;
                        } else {
                            repl_result.push('$');
                            ri += 1;
                        }
                    } else if repl_chars[ri + 1] == '$' {
                        repl_result.push('$');
                        ri += 2;
                    } else if repl_chars[ri + 1].is_ascii_digit() {
                        // Numbered group: $1, $2, etc.
                        let digit = (repl_chars[ri + 1] as u32 - '0' as u32) as usize;
                        if let Some(m) = caps.get(digit) {
                            repl_result.push_str(m.as_str());
                        }
                        ri += 2;
                    } else {
                        repl_result.push('$');
                        ri += 1;
                    }
                } else {
                    repl_result.push(repl_chars[ri]);
                    ri += 1;
                }
            }

            result.push_str(&repl_result);
            last_end = full_match.end();
        }

        result.push_str(&str_data[last_end..]);
        js_string_from_str(&result)
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
