//! RegExp runtime support for Perry
//!
//! Provides JavaScript-compatible regular expression operations using the Rust regex crate.
//! RegExp objects are heap-allocated and store the compiled pattern and flags.

#[cfg(feature = "regex-engine")]
use regex::Regex;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ptr;
#[cfg(feature = "regex-engine")]
use std::sync::Arc;

#[cfg(feature = "regex-engine")]
use crate::array::ArrayHeader;
use crate::string::StringHeader;
#[cfg(feature = "regex-engine")]
use crate::value::js_nanbox_string;

use crate::object::ObjectHeader;

/// The compiled standard-engine regex type. When the regex engine is gated
/// off, `RegExpHeader::regex_ptr` is typed `*mut ()` (a never-dereferenced
/// dangling field) so the identity/display layer keeps the same struct
/// layout without pulling in the `regex` crate.
#[cfg(feature = "regex-engine")]
type CompiledRegex = regex::Regex;
#[cfg(not(feature = "regex-engine"))]
type CompiledRegex = ();

#[cfg(feature = "regex-engine")]
mod class_range_validate;
#[cfg(feature = "regex-engine")]
mod compile;
mod escape;
#[cfg(feature = "regex-engine")]
mod exec_array;
#[cfg(feature = "regex-engine")]
mod grammar;
#[cfg(feature = "regex-engine")]
mod match_all;
#[cfg(feature = "regex-engine")]
mod replace_expand;
mod replace_fn;
#[cfg(feature = "regex-engine")]
mod unicode17;
#[cfg(feature = "regex-engine")]
mod unicode17_data;
mod utf16;
#[cfg(feature = "regex-engine")]
use class_range_validate::has_out_of_order_double_dash_class_range;
#[cfg(feature = "regex-engine")]
pub use compile::js_regexp_compile_value;
pub use escape::js_regexp_escape;
#[cfg(feature = "regex-engine")]
use exec_array::{
    byte_index_to_utf16_index, set_exec_array_groups, set_exec_array_indices,
    set_exec_array_indices_fancy, set_exec_array_metadata, set_exec_array_metadata_value,
    utf16_index_to_byte,
};
#[cfg(feature = "regex-engine")]
use grammar::{
    has_invalid_repeated_quantifier, has_unicode_forbidden_legacy_escape,
    has_unicode_forbidden_pattern, js_regex_to_rust,
};
#[cfg(feature = "regex-engine")]
pub use match_all::{
    dispatch_regexp_string_iterator_method, js_string_match_all, js_string_match_all_value,
};

/// Class id for `RegExp String Iterator` exotic objects. Referenced by the
/// always-linked iterator-prototype dispatch, so it stays ungated even when
/// the regex engine (which produces these iterators) is compiled out.
pub const REGEXP_STRING_ITERATOR_CLASS_ID: u32 = 0xFFFF_000A;
#[cfg(feature = "regex-engine")]
use replace_expand::expand_js_replacement;
#[cfg(feature = "regex-engine")]
pub use replace_expand::{
    js_string_replace_all_regex_fn, js_string_replace_all_regex_named, js_string_replace_regex_fn,
    js_string_replace_regex_named,
};
#[cfg(feature = "regex-engine")]
use replace_fn::call_replace_callback;
pub use replace_fn::{
    js_string_replace_all_string, js_string_replace_all_string_fn, js_string_replace_string,
    js_string_replace_string_fn,
};
#[cfg(feature = "regex-engine")]
mod exec;
#[cfg(feature = "regex-engine")]
mod match_string;
#[cfg(feature = "regex-engine")]
pub use exec::js_regexp_exec;
#[cfg(feature = "regex-engine")]
pub use match_string::{js_string_match, js_string_match_value, js_string_search_value};

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
    // Wall 18: check the header-resident magic FIRST so identity survives a
    // duplicate-runtime thread-local split (see `RegExpHeader.magic`). A
    // RegExp is a `gc_malloc(GC_TYPE_OBJECT)` allocation, so it always carries
    // a preceding GcHeader; only read the magic field when the GC header says
    // this is an object of sufficient size to actually contain it.
    if regex_header_has_magic(ptr as *const RegExpHeader) {
        return true;
    }
    REGEX_POINTERS.with(|s| s.borrow().contains(&(ptr as usize)))
}

/// Bounds-checked read of `RegExpHeader.magic`. Confirms the preceding
/// `GcHeader` exists, is a `GC_TYPE_OBJECT`, and the allocation is large enough
/// to hold a full `RegExpHeader` before dereferencing the `magic` field.
/// Returns true iff the field equals [`REGEXP_MAGIC`]. Immune to which linked
/// `perry-runtime` copy's thread-locals are live.
///
/// SAFETY: this is called from `is_regex_pointer` / `is_registered_regex` with
/// ARBITRARY payloads — including small-handle-band ids (`< 0x100000`), null,
/// NaN-box tag remnants, and small-buffer slab addresses that carry NO
/// `GcHeader`. Dereferencing `addr - GC_HEADER_SIZE` directly SIGSEGVs on those
/// (regression caught by `object_to_string_rejects_handle_band_ids`). Route the
/// header read through [`addr_class::try_read_gc_header`], which magnitude-
/// classifies FIRST (rejecting the handle band + implausible heap addresses +
/// slab addresses) and only then touches memory.
#[inline]
pub(crate) fn regex_header_has_magic(re: *const RegExpHeader) -> bool {
    let addr = re as usize;
    unsafe {
        let Some(gc) = crate::value::addr_class::try_read_gc_header(addr) else {
            return false;
        };
        if gc.obj_type != crate::gc::GC_TYPE_OBJECT {
            return false;
        }
        // `size` in the GcHeader covers the GcHeader + payload. Require enough
        // payload to reach the `magic` field.
        if (gc.size as usize) < crate::gc::GC_HEADER_SIZE + std::mem::size_of::<RegExpHeader>() {
            return false;
        }
        (*re).magic == REGEXP_MAGIC
    }
}

/// The GC-VISIBLE slots of a `RegExpHeader`. Only three fields can hold a
/// heap reference the collector must mark/relocate:
///   * `pattern_ptr` — the original-source `StringHeader`,
///   * `flags_ptr`   — the flags `StringHeader`,
///   * `last_index`  — a writable JSValue (`re.lastIndex = …`) that may be a
///     NaN-boxed heap pointer.
/// `regex_ptr`/`fancy_ptr` point to OFF-heap leaked Rust allocations and the
/// bool/`magic` fields are never heap refs, so they must NOT be scanned.
///
/// `pattern_ptr` and `flags_ptr` are consecutive equal-width fields, so under
/// `#[repr(C)]` they are adjacent and form a 2-slot contiguous range; the
/// returned tuple is `(range_start, range_slot_count, last_index_slot)`. Offsets
/// are taken from the actual struct via `addr_of_mut!` (no hardcoded layout).
#[inline]
pub(crate) unsafe fn regex_gc_slot_ptrs(re: *mut RegExpHeader) -> (*mut u64, usize, *mut u64) {
    let pattern = std::ptr::addr_of_mut!((*re).pattern_ptr) as *mut u64;
    let flags = std::ptr::addr_of_mut!((*re).flags_ptr) as *mut u64;
    let last_index = std::ptr::addr_of_mut!((*re).last_index) as *mut u64;
    // `pattern_ptr` then `flags_ptr` must be adjacent for the 2-slot range to be
    // exact; assert so a future field reorder is caught in debug builds.
    debug_assert_eq!(flags as usize - pattern as usize, 8);
    (pattern, 2, last_index)
}

#[cfg(feature = "regex-engine")]
thread_local! {
    /// Cache of compiled regex objects, keyed by (pattern, flags).
    static REGEX_CACHE: RefCell<HashMap<(String, String), Arc<Regex>>> = RefCell::new(HashMap::new());
    /// Fancy-regex fallback cache for patterns with lookbehind/lookahead.
    static FANCY_CACHE: RefCell<HashMap<(String, String), Arc<fancy_regex::Regex>>> = RefCell::new(HashMap::new());
}

/// Compiled-program size budget handed to both regex engines.
///
/// The `regex` crate (and the `regex-automata` backend `fancy-regex`
/// delegates to) caps a compiled program at 10 MiB by default and rejects
/// anything larger with `CompiledTooBig` / `ExceededSizeLimit` — which our
/// callers surface as a bogus `SyntaxError: invalid pattern`. JS itself has
/// no such limit, so a *valid* pattern with large bounded repetitions is
/// wrongly rejected. semver's ReDoS-hardened `safeRe` rewrites (`\s{0,1}`,
/// `\d{1,256}`, `[…]{0,250}`, …) blow well past 10 MiB; raise the budget so
/// these legitimate patterns compile. 64 MiB comfortably fits semver's full
/// range regex while still bounding pathological input.
#[cfg(feature = "regex-engine")]
const REGEX_SIZE_LIMIT: usize = 64 * 1024 * 1024;

/// Build a `regex` crate `Regex` with the raised [`REGEX_SIZE_LIMIT`] so that
/// large-but-valid bounded-quantifier patterns aren't rejected as
/// `CompiledTooBig`. Drop-in replacement for `regex::Regex::new`.
#[cfg(feature = "regex-engine")]
pub(crate) fn build_std_regex(pattern: &str) -> Result<Regex, regex::Error> {
    regex::RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()
}

/// Build a `fancy_regex` `Regex` with the raised delegate size limit (see
/// [`REGEX_SIZE_LIMIT`]). `fancy-regex` delegates non-fancy subpatterns to the
/// `regex` crate, so the same 10 MiB cap applies there; raise it in lockstep.
#[cfg(feature = "regex-engine")]
pub(crate) fn build_fancy_regex(pattern: &str) -> Result<fancy_regex::Regex, fancy_regex::Error> {
    fancy_regex::RegexBuilder::new(pattern)
        .delegate_size_limit(REGEX_SIZE_LIMIT)
        .build()
}

/// Entry cap for `REGEX_CACHE`/`FANCY_CACHE` (2026-07-09 GC audit: one entry
/// per distinct `(pattern, flags)` ever compiled, no cap of any kind, entries
/// up to [`REGEX_SIZE_LIMIT`] — `new RegExp(userInput)` was an attacker-driven
/// OOM). When an insert would exceed the cap the whole map is cleared — the
/// `PARSE_KEY_CACHE` precedent: cheap, no LRU bookkeeping, recompilation is
/// the fallback. Live `RegExpHeader`s are unaffected: each header OWNS a
/// leaked `Arc` reference to its compiled program(s) (`regex_ptr`/`fancy_ptr`),
/// so dropping the cache's references cannot free a program still in use.
#[cfg(feature = "regex-engine")]
const REGEX_CACHE_MAX_ENTRIES: usize = 512;

/// Clear-on-overflow guard shared by both compiled-regex caches: make room
/// for one more entry, wiping the map when it is at capacity.
#[cfg(feature = "regex-engine")]
fn evict_regex_cache_if_full<V>(cache: &mut HashMap<(String, String), V>) {
    if cache.len() >= REGEX_CACHE_MAX_ENTRIES {
        cache.clear();
    }
}

#[cfg(feature = "regex-engine")]
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
        let regex = match build_std_regex(&regex_pattern) {
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
                    if let Ok(fre) = build_fancy_regex(&regex_pattern) {
                        let mut fc = fc.borrow_mut();
                        evict_regex_cache_if_full(&mut fc);
                        fc.insert(
                            (pattern.to_string(), flags.to_string()),
                            std::sync::Arc::new(fre),
                        );
                    }
                });
                Regex::new(r"[^\s\S]").unwrap()
            }
        };
        let arc = Arc::new(regex);
        evict_regex_cache_if_full(&mut cache);
        cache.insert((pattern.to_string(), flags.to_string()), arc.clone());
        arc
    })
}

/// Header for heap-allocated RegExp objects
#[repr(C)]
pub struct RegExpHeader {
    /// Pointer to the compiled Regex object (boxed). Typed via the
    /// `CompiledRegex` alias so the struct layout is identical whether or not
    /// the regex engine is linked (it's `*mut ()` when gated off and never
    /// dereferenced — all dereferencing sites are themselves engine-gated).
    regex_ptr: *mut CompiledRegex,
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
    /// Wall 18 (nestjs / get-intrinsic): self-identifying sentinel.
    ///
    /// `is_valid_regex_ptr` / `is_regex_pointer` / `is_registered_regex` used to
    /// rely SOLELY on the `REGEX_POINTERS` thread-local set. That breaks when a
    /// statically-linked app pulls a second copy of `perry-runtime` (every
    /// `perry-ext-*` archive bundles its own — the link emits duplicate-symbol
    /// warnings): `js_regexp_new` inserts into copy-A's thread-local while the
    /// `.source`/`.flags`/dynamic-`.replace` reader resolves to copy-B's
    /// (empty) thread-local, so a perfectly valid regex reports `.source ===
    /// "(?:)"`, `is_regex_pointer === false`, and `str.replace(re, fn)` (via a
    /// `function-bind` bound `String.prototype.replace`) treats `re` as a plain
    /// string pattern → never matches → get-intrinsic's `stringToPath` returns
    /// `[]` → `intrinsic %% does not exist!` → express adapter load `exit(1)`.
    ///
    /// Storing the marker (and the fancy-regex Arc) ON the heap header makes
    /// identity + fancy-fallback resolution independent of WHICH runtime copy's
    /// thread-locals are live. Set to `REGEXP_MAGIC` by `js_regexp_new`.
    pub magic: u64,
    /// Leaked `Arc<fancy_regex::Regex>` (as a raw pointer) for patterns the
    /// `regex` crate can't compile (lookahead/lookbehind/backrefs), or null.
    /// Header-resident twin of the `FANCY_CACHE` thread-local so the fancy
    /// fallback survives the duplicate-runtime split described above.
    pub fancy_ptr: *const (),
}

/// Self-identifying sentinel stamped into every `RegExpHeader.magic` by
/// `js_regexp_new`. ASCII `"PRYREGEX"` little-endian — distinctive enough that
/// a random heap object is astronomically unlikely to collide.
pub const REGEXP_MAGIC: u64 = 0x5845_4745_5259_5250;

/// `ToLength(Get(R, "lastIndex"))` → a non-negative integer match offset. The
/// stored value may be any JSValue (e.g. `re.lastIndex = { valueOf() {…} }`), so
/// coerce via `ToNumber` (which invokes `valueOf`/`toString`), then `ToInteger`,
/// clamped to ≥ 0.
#[cfg(feature = "regex-engine")]
pub(crate) fn regex_last_index_offset(re: *const RegExpHeader) -> usize {
    let stored = f64::from_bits(unsafe { (*re).last_index });
    let n = crate::builtins::js_number_coerce(stored);
    if n.is_nan() || n <= 0.0 {
        0
    } else {
        n.floor() as usize
    }
}

#[cfg(feature = "regex-engine")]
#[inline]
pub(crate) fn store_last_index_number(re: *mut RegExpHeader, n: usize) {
    unsafe {
        (*re).last_index = crate::value::JSValue::number(n as f64).bits();
    }
}

/// Spec `Set(R, "lastIndex", n, true)` — the lastIndex updates in
/// RegExpBuiltinExec (steps 14/18) are performed with the *Throw* flag set.
/// A user can make `lastIndex` non-writable
/// (`Object.defineProperty(re, "lastIndex", { writable: false })`); the
/// throwing setter then raises a `TypeError` rather than silently dropping the
/// write (test262 prototype/{exec,test}/y-fail-lastindex-no-write). When
/// `lastIndex` is writable (the default) this just stores the number.
#[cfg(feature = "regex-engine")]
pub(crate) fn set_last_index_throwing(re: *mut RegExpHeader, n: usize) {
    let writable = crate::object::get_property_attrs(re as usize, "lastIndex")
        .map(|a| a.writable())
        .unwrap_or(true);
    if !writable {
        let message = b"Cannot assign to read only property 'lastIndex' of object";
        let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
        let err = crate::error::js_typeerror_new(msg);
        crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
    }
    store_last_index_number(re, n);
}

/// Check if a pointer is valid (not null and not a small invalid value from bad NaN-unboxing)
#[inline]
pub(crate) fn is_valid_ptr<T>(p: *const T) -> bool {
    !p.is_null() && (p as usize) >= 0x1000
}

/// Check if a RegExpHeader pointer is legitimate — it must point to a
/// header we allocated via `js_regexp_new` (tracked in REGEX_POINTERS).
/// The LLVM backend's `new RegExp(pat, flags)` currently falls through
/// to the generic `lower_new` path which allocates an empty object and
/// NaN-boxes it as a regex; subsequent `.exec()` / `.test()` calls would
/// read garbage from that object if we didn't gate them on this check.
#[inline]
pub(crate) fn is_valid_regex_ptr(p: *const RegExpHeader) -> bool {
    if !is_valid_ptr(p) {
        return false;
    }
    // Wall 18: header magic first (duplicate-runtime thread-local resilient).
    if regex_header_has_magic(p) {
        return true;
    }
    REGEX_POINTERS.with(|s| s.borrow().contains(&(p as usize)))
}

/// Public: is `addr` a RegExpHeader we allocated via `js_regexp_new`?
/// Used by the console/`util.inspect` formatter to print regex literals
/// as `/source/flags` instead of `{}` (they're GC_TYPE_OBJECT allocations
/// with no enumerable string keys). Registry-gated so a generic object
/// is never mis-read as a RegExpHeader.
pub fn is_registered_regex(addr: usize) -> bool {
    // Wall 18: header magic first (duplicate-runtime thread-local resilient).
    if regex_header_has_magic(addr as *const RegExpHeader) {
        return true;
    }
    REGEX_POINTERS.with(|s| s.borrow().contains(&addr))
}

/// Internal helper: Get string data from StringHeader
pub(crate) fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, len);
        std::str::from_utf8_unchecked(bytes)
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
pub(super) fn js_string_from_str(s: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

#[cfg(feature = "regex-engine")]
fn throw_replace_all_non_global_regex() -> ! {
    let message = b"String.prototype.replaceAll called with a non-global RegExp argument";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(feature = "regex-engine")]
fn throw_match_all_non_global_regex() -> ! {
    let message = b"String.prototype.matchAll called with a non-global RegExp argument";
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

#[cfg(feature = "regex-engine")]
#[inline]
fn ensure_replace_all_regex_global(re: *const RegExpHeader) {
    unsafe {
        if !(*re).global {
            throw_replace_all_non_global_regex();
        }
    }
}

/// Throw a `SyntaxError` with the given message and never return.
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
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
    //
    // PERF (#5777 follow-up): the ENTIRE validation block is gated on a
    // REGEX_CACHE miss. Regex validity is a pure function of (pattern, flags):
    // an invalid pattern throws here BEFORE `get_or_compile_regex` can ever
    // cache it, and both writers of REGEX_CACHE — this function and
    // `regex/compile.rs` (`RegExp.prototype.compile`) — run these exact checks
    // first, so any entry already in the cache is provably valid and
    // re-validating it can only burn CPU. #5777 already skipped the expensive
    // both-engines recompile on a hit; this extends the skip to the "cheap"
    // JS-syntax checks too, which are not actually cheap:
    // `has_invalid_repeated_quantifier` does a
    // `pattern.chars().collect::<Vec<char>>()` (a ~51 KB allocation for a
    // 12,807-char pattern) plus an O(n) scan on EVERY `new RegExp(...)`. The
    // common `string-width`/`emoji-regex` npm packages construct a fresh
    // ~12,807-char `/…/g` literal on every measurement and a layout pass can
    // call them thousands of times, so this re-validation — not the
    // already-cached compile — became the top hot frame in profiles.
    {
        let in_cache = REGEX_CACHE.with(|c| {
            c.borrow()
                .contains_key(&(pattern_str.to_string(), flags_str.to_string()))
        });
        if !in_cache {
            if has_invalid_repeated_quantifier(pattern_str) {
                throw_regexp_syntax_error(&format!(
                    "Invalid regular expression: /{}/: invalid pattern",
                    pattern_str
                ));
            }
            // `--` is the real ClassSetExpression subtraction operator under
            // the `v` flag (UTS #51) — `[a--z]` there means "a minus z", not
            // a malformed range — so only legacy/`u`-mode patterns are
            // subject to the doubled-hyphen range-order check.
            if !flags_str.contains('v') && has_out_of_order_double_dash_class_range(pattern_str) {
                throw_regexp_syntax_error(&format!(
                    "Invalid regular expression: /{}/: invalid pattern",
                    pattern_str
                ));
            }
            // Annex B.1.4 legacy escapes (`\1` non-backref octal, `\0DD`, `\8`/`\9`,
            // `\c` without a control letter) are accepted in sloppy patterns but are
            // a hard SyntaxError under the `/u` (and `/v`) flag — `js_regex_to_rust`
            // would otherwise silently relax them. (test262 RegExp/
            // unicode_restricted_octal_escape + unicode_restricted_identity_escape_c)
            if unicode && has_unicode_forbidden_legacy_escape(pattern_str) {
                throw_regexp_syntax_error(&format!(
                    "Invalid regular expression: /{}/: invalid pattern",
                    pattern_str
                ));
            }
            // The remaining Annex B.1.4 leniencies (lone `]`/`}`, incomplete `{`
            // quantifiers, `\d`-style range endpoints, quantified lookarounds, and
            // forbidden IdentityEscapes) are likewise hard errors under `/u`. Gated
            // on `u` specifically — `/v`'s ClassSetExpression grammar differs.
            if flags_str.contains('u') && has_unicode_forbidden_pattern(pattern_str) {
                throw_regexp_syntax_error(&format!(
                    "Invalid regular expression: /{}/: invalid pattern",
                    pattern_str
                ));
            }
            // The expensive part of validation: compile the pattern with both
            // engines just to confirm it is well-formed.
            let translated = js_regex_to_rust(pattern_str);
            if build_std_regex(&translated).is_err() && build_fancy_regex(&translated).is_err() {
                throw_regexp_syntax_error(&format!(
                    "Invalid regular expression: /{}/: invalid pattern",
                    pattern_str
                ));
            }
        }
    }

    // Get or compile the regex from the cache. The header OWNS a leaked `Arc`
    // reference (`Arc::into_raw`) to the compiled program — mirroring
    // `fancy_ptr` below — so the pointer stays valid even after the capped
    // `REGEX_CACHE` (see `REGEX_CACHE_MAX_ENTRIES`) evicts its own reference.
    // Previously this borrowed `Arc::as_ptr` and relied on the cache never
    // dropping an entry.
    let arc = get_or_compile_regex(pattern_str, flags_str);
    let regex_ptr = Arc::into_raw(arc) as *mut Regex;

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
            // #5067 — catchable RangeError instead of aborting on OOM.
            crate::error::throw_allocation_failed();
        }
        let ptr = raw as *mut RegExpHeader;
        // A previous (collected) RegExp at this address may have left expando
        // properties in the side table; a fresh RegExp must start clean.
        crate::object::exotic_expando::expando_clear_on_alloc(ptr as usize);

        (*ptr).regex_ptr = regex_ptr;
        (*ptr).pattern_ptr = pattern;
        (*ptr).flags_ptr = canonical_flags_ptr;
        // `pattern_ptr` / `flags_ptr` are GC-managed StringHeaders — the GC scans
        // this 2-slot payload range via the magic-tagged RegExp layout, and
        // `canonical_flags_ptr` (js_string_from_str above) is a freshly-allocated
        // YOUNG string. They are stored into this malloc'd (old-generation) header
        // by raw writes; without a write barrier the old→young edge is never
        // remembered, so a copying minor GC sweeps the string while the retained
        // RegExp still points at it. The evacuation verifier reports this as an
        // uncovered object→string edge, and it crashes for real when the freed
        // slot is later scanned/read (a heavy regex workload — e.g. ANSI/emoji
        // parsing in a terminal UI — hits it within seconds). Remember both edges,
        // mirroring every other native-header pointer store (closure captures,
        // object prototype slots, array headers). `runtime_write_barrier_gc_slot`
        // detects the malloc parent and only remembers genuinely-young children,
        // so an already-old/interned `pattern` is a harmless no-op.
        let regexp_parent_addr = ptr as usize;
        if !pattern.is_null() {
            crate::gc::runtime_write_barrier_gc_slot(
                regexp_parent_addr,
                std::ptr::addr_of!((*ptr).pattern_ptr) as usize,
                js_nanbox_string(pattern as i64).to_bits(),
            );
        }
        if !canonical_flags_ptr.is_null() {
            crate::gc::runtime_write_barrier_gc_slot(
                regexp_parent_addr,
                std::ptr::addr_of!((*ptr).flags_ptr) as usize,
                js_nanbox_string(canonical_flags_ptr as i64).to_bits(),
            );
        }
        (*ptr).case_insensitive = case_insensitive;
        (*ptr).global = global;
        (*ptr).multiline = multiline;
        (*ptr).sticky = sticky;
        (*ptr).dot_all = dot_all;
        (*ptr).unicode = unicode;
        (*ptr).has_indices = has_indices;
        (*ptr).last_index = crate::value::JSValue::number(0.0).bits();
        // Wall 18: self-identifying marker so identity checks survive a
        // duplicate-runtime thread-local split.
        (*ptr).magic = REGEXP_MAGIC;
        // Header-resident fancy-regex fallback (lookahead/lookbehind/backrefs)
        // so `.replace(re, fn)` etc. don't depend on the (possibly other-copy)
        // FANCY_CACHE thread-local. `get_or_compile_regex` above already
        // populated FANCY_CACHE on THIS thread when the std `regex` crate
        // rejected the pattern; clone that Arc onto the header (leaked so the
        // raw pointer stays valid for the header's lifetime — RegExp headers
        // and their compiled programs live for the process today).
        (*ptr).fancy_ptr = FANCY_CACHE.with(|fc| {
            match fc
                .borrow()
                .get(&(pattern_str.to_string(), flags_str.to_string()))
            {
                Some(arc) => Arc::into_raw(arc.clone()) as *const (),
                None => std::ptr::null(),
            }
        });

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
#[cfg(feature = "regex-engine")]
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

/// `RegExp(...)` invoked as a *function* (not `new`). ECMA-262 22.2.4.1 step 2:
/// when `NewTarget` is undefined, `pattern` is a RegExp and `flags` is
/// `undefined`, and `pattern.constructor` is the `RegExp` intrinsic, the call
/// returns `pattern` **unchanged** (object identity) instead of constructing a
/// copy. So `var r = /x/i; RegExp(r) === r` is `true`, and a property added to
/// `r` is visible through the returned reference (test262
/// `built-ins/RegExp/S15.10.3.1_A1_T*`, #5586).
///
/// Perry models no user-visible RegExp subclassing, so a registered RegExp's
/// `constructor` resolves through `RegExp.prototype` to the intrinsic `RegExp`
/// and the `SameValue` check holds — *unless* user code has installed an own
/// `constructor` property (e.g. `re.constructor = null`), which makes the
/// `SameValue` check fail and forces a fresh copy
/// (`built-ins/RegExp/call_with_regexp_not_same_constructor.js`). Every other
/// shape (string/object/undefined pattern, or any non-`undefined` flags —
/// which forces a fresh copy with the new flags) likewise falls through to the
/// general [`js_regexp_construct`] path.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_construct_call(pattern: f64, flags: f64) -> *mut RegExpHeader {
    let pv = crate::value::JSValue::from_bits(pattern.to_bits());
    let fv = crate::value::JSValue::from_bits(flags.to_bits());
    if fv.is_undefined() && pv.is_pointer() {
        let addr = pv.as_pointer::<u8>() as usize;
        if is_registered_regex(addr)
            // IsRegExp(pattern): the shortcut is gated on `IsRegExp`, which first
            // consults `pattern[@@match]` — a registered RegExp with an own
            // `re[Symbol.match] = false` is NOT regexp-like and must copy
            // (`built-ins/RegExp/call_with_regexp_match_falsy.js`). Only when
            // `@@match` is absent does it fall back to the [[RegExpMatcher]] slot
            // (which every registered RegExp has).
            && regexp_pattern_is_regexp_like(pattern)
            // SameValue(RegExp, pattern.constructor): the identity shortcut only
            // applies while `constructor` is still the inherited intrinsic. An
            // own `constructor` override (the only way it can differ here) must
            // copy instead.
            && crate::object::exotic_expando::value_lookup(
                crate::object::exotic_expando::ExoticKind::RegExp,
                addr,
                "constructor",
            )
            .is_none()
        {
            return pv.as_pointer::<RegExpHeader>() as *mut RegExpHeader;
        }
    }
    js_regexp_construct(pattern, flags)
}

/// `IsRegExp(pattern)` for an already-registered RegExp header: consult an own
/// `pattern[@@match]` override (decisive via `ToBoolean`) and only fall back to
/// the [[RegExpMatcher]] slot — which a registered RegExp always has — when no
/// `@@match` property is present. Used to gate the `RegExp(re)` identity
/// shortcut so `re[Symbol.match] = false` correctly forces a fresh copy.
#[cfg(feature = "regex-engine")]
fn regexp_pattern_is_regexp_like(pattern: f64) -> bool {
    let match_sym = crate::symbol::well_known_symbol("match");
    if match_sym.is_null() {
        return true;
    }
    let sym_val = f64::from_bits(crate::value::JSValue::pointer(match_sym as *const u8).bits());
    let m = unsafe { crate::symbol::js_object_get_symbol_property(pattern, sym_val) };
    if crate::value::JSValue::from_bits(m.to_bits()).is_undefined() {
        // No own/inherited @@match override → registered RegExp ([[RegExpMatcher]]).
        true
    } else {
        crate::value::js_is_truthy(m) != 0
    }
}

/// Test if a string matches the regex pattern
/// regex.test(string) -> boolean
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
pub(crate) fn lookup_fancy_regex(re: *const RegExpHeader) -> Option<Arc<fancy_regex::Regex>> {
    unsafe {
        // Wall 18: header-resident fancy Arc first (duplicate-runtime
        // thread-local resilient). `fancy_ptr` is a leaked `Arc` raw pointer; to
        // hand back an owned `Arc` clone WITHOUT consuming the header's
        // reference, reconstruct, clone, then `mem::forget` the reconstructed
        // one so the header's strong count is preserved.
        if regex_header_has_magic(re) && !(*re).fancy_ptr.is_null() {
            let raw = (*re).fancy_ptr as *const fancy_regex::Regex;
            let arc = Arc::from_raw(raw);
            let cloned = arc.clone();
            std::mem::forget(arc);
            return Some(cloned);
        }
        let pat = string_as_str((*re).pattern_ptr);
        let flags_str = string_as_str((*re).flags_ptr);
        FANCY_CACHE.with(|fc| {
            fc.borrow()
                .get(&(pat.to_string(), flags_str.to_string()))
                .cloned()
        })
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
#[cfg(feature = "regex-engine")]
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
            b'<' if has_named_groups => {
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
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_string_split_regex(
    s: *const StringHeader,
    re: *const RegExpHeader,
) -> *mut ArrayHeader {
    js_string_split_regex_n(s, re, -1)
}

/// string.split(regex, limit) — limit<0 means no limit, limit==0 means empty
/// (issue #567).
#[cfg(feature = "regex-engine")]
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

    const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
    unsafe {
        // Each element is either a substring (`Some`) or `undefined` (`None`,
        // for an unmatched capture group spliced into the result).
        let parts: Vec<Option<String>> = if let Some(fre) = lookup_fancy_regex(re) {
            // Fancy-regex fallback (lookbehind/backreferences): `fancy_regex` has
            // no `split`, so walk non-overlapping matches and slice between them.
            // (Captured-group splicing is not reproduced for this engine.)
            let mut v: Vec<Option<String>> = Vec::new();
            let mut last = 0usize;
            let mut iter = fre.find_iter(&str_data);
            while let Some(Ok(m)) = iter.next() {
                v.push(Some(str_data[last..m.start()].to_string()));
                last = m.end();
            }
            v.push(Some(str_data[last..].to_string()));
            if limit > 0 && (v.len() as i64) > (limit as i64) {
                v.truncate(limit as usize);
            }
            v
        } else {
            // Standard engine: the JS `RegExp.prototype[Symbol.split]` algorithm
            // (21.2.5.11). The `regex` crate's own `split` diverges from JS for
            // zero-width matches (it emits leading/trailing/consecutive empty
            // strings the spec's `e == p` skip suppresses) and never splices
            // captured groups, so walk the string the spec's way instead.
            crate::string::spec_regex_split(&*(*re).regex_ptr, &str_data, limit)
        };

        let arr = crate::array::js_array_alloc(parts.len() as u32);
        let scope = crate::gc::RuntimeHandleScope::new();
        let arr_handle = scope.root_raw_mut_ptr(arr);
        (*arr_handle.get_raw_mut_ptr::<ArrayHeader>()).length = parts.len() as u32;

        for (i, part) in parts.iter().enumerate() {
            let nanboxed = match part {
                Some(text) => {
                    let str_ptr = js_string_from_str(text) as u64;
                    STRING_TAG | (str_ptr & POINTER_MASK)
                }
                None => TAG_UNDEFINED,
            };
            let arr = arr_handle.get_raw_mut_ptr::<ArrayHeader>();
            // GC_STORE_AUDIT(BARRIERED): regex split result slot uses the shared array slot-store helper.
            crate::array::store_array_slot(arr, i, nanboxed);
        }
        arr_handle.get_raw_mut_ptr::<ArrayHeader>()
    }
}

/// Search for a regex match in a string
/// string.search(regex) -> number (index of first match, -1 if none)
#[cfg(feature = "regex-engine")]
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
                Ok(Some(m)) => byte_index_to_utf16_index(str_data, m.start()) as i32,
                _ => -1,
            };
        }

        let regex = &*(*re).regex_ptr;
        match regex.find(str_data) {
            Some(m) => {
                // `String.prototype.search` returns a JS string index — UTF-16
                // code units, matching `.index` / `lastIndex` / `str.length`.
                byte_index_to_utf16_index(str_data, m.start()) as i32
            }
            None => -1,
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
#[cfg(feature = "regex-engine")]
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
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_exec_get_index() -> f64 {
    LAST_EXEC_INDEX.with(|idx| *idx.borrow())
}

/// Get the .groups object from the last exec() call
/// Returns I64 pointer (0 for no groups)
#[cfg(feature = "regex-engine")]
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
        visitor.visit_raw_mut_ptr_slot(&mut g.borrow_mut());
    });
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_set_last_exec_groups(ptr: *mut ObjectHeader) {
    LAST_EXEC_GROUPS.with(|g| {
        *g.borrow_mut() = ptr;
    });
}

#[cfg(all(test, feature = "regex-engine"))]
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

#[cfg(all(test, feature = "regex-engine"))]
mod tests;
