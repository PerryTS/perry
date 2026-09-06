//! RegExp runtime support for Perry
//!
//! Provides JavaScript-compatible regular expression operations using the Rust regex crate.
//! RegExp objects are heap-allocated and store the compiled pattern and flags.

#[cfg(feature = "regex-engine")]
use regex::Regex;
use std::cell::RefCell;
// Every use of `HashMap` in this file is inside a `#[cfg(feature = "regex-engine")]`
// block, so an unconditional import is an unused-import error under the
// `warnings` job's `-D warnings` when `perry`'s own binaries pull the runtime
// in without that feature.
#[cfg(feature = "regex-engine")]
use std::collections::HashMap;
use std::ptr;
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
mod flags;
#[cfg(feature = "regex-engine")]
mod program_key;
#[cfg(feature = "regex-engine")]
mod replace_expand_fancy;
#[cfg(feature = "regex-engine")]
pub(crate) use program_key::{ProgramKey, NEVER_MATCH_PATTERN};
#[cfg(feature = "regex-engine")]
pub use replace_expand_fancy::{
    js_string_replace_all_regex, js_string_replace_regex, js_string_search_regex,
    js_string_split_regex, js_string_split_regex_n,
};
#[cfg(feature = "regex-engine")]
mod global_guards;
#[cfg(feature = "regex-engine")]
mod global_scan;
#[cfg(feature = "regex-engine")]
mod grammar;
#[cfg(feature = "regex-engine")]
mod lazy;
#[cfg(feature = "regex-engine")]
mod match_all;
#[cfg(feature = "regex-engine")]
mod repeat_matcher;
#[cfg(feature = "regex-engine")]
mod replace_expand;
mod replace_fn;
#[cfg(feature = "regex-engine")]
mod site_cache;
/// Literal-site keyed construction cache — identity by an immortal address
/// emitted per regex literal, so a hit costs one word compare instead of a
/// fingerprint plus a full byte compare of the pattern.
#[cfg(feature = "regex-engine")]
mod site_key;
#[cfg(feature = "regex-engine")]
mod unicode17;
#[cfg(feature = "regex-engine")]
mod unicode17_data;
mod utf16;
#[cfg(feature = "regex-engine")]
use class_range_validate::has_out_of_order_double_dash_class_range;
#[cfg(feature = "regex-engine")]
pub use compile::js_regexp_compile_value;
use escape::escape_regexp_source;
pub use escape::js_regexp_escape;
#[cfg(feature = "regex-engine")]
use exec_array::{
    byte_index_to_utf16_index, materialize_exec_match, materialize_match_list,
    set_exec_array_metadata_value, utf16_index_to_byte, OwnedCapture, OwnedExecMatch,
};
#[cfg(feature = "regex-engine")]
use flags::validate_and_canonicalize_flags;
#[cfg(feature = "regex-engine")]
use global_guards::{ensure_replace_all_regex_global, throw_match_all_non_global_regex};
#[cfg(feature = "regex-engine")]
use grammar::{
    collapse_redos_guard_quantifiers, has_invalid_repeated_quantifier,
    has_unicode_forbidden_legacy_escape, has_unicode_forbidden_pattern, js_regex_to_rust,
};
#[cfg(feature = "regex-engine")]
pub(crate) use match_all::dispatch_regexp_string_iterator_method_builtin;
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
use replace_fn::{call_replace_callback, copy_replace_source, finish_replace_bytes};
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

crate::perry_thread_local! {
    #[cfg(feature = "regex-engine")]
    static LAST_EXEC_INDEX: RefCell<f64> = const { RefCell::new(0.0) };

    static LAST_EXEC_GROUPS: RefCell<*mut ObjectHeader> = const { RefCell::new(ptr::null_mut()) };

    /// Set of live RegExpHeader pointers allocated in this thread.
    /// Used by callers (e.g. `js_string_split`) to distinguish a regex
    /// delimiter from a string delimiter when the codegen can't tell
    /// statically. GC move/death hooks rekey and remove entries as cells
    /// relocate or die. Header magic remains the primary identity check.
    static REGEX_POINTERS: RefCell<crate::fast_hash::PtrHashSet<usize>> = RefCell::new(crate::fast_hash::new_ptr_hash_set());

    /// Issue #637: Owned copies of pattern and flags strings keyed by
    /// the RegExpHeader pointer. The header's `pattern_ptr` / `flags_ptr`
    /// fields hold raw `*const StringHeader` pointers to the input
    /// strings — when those inputs are temporaries (e.g. the result of
    /// a template-literal expression `\`^${p}\``), the GC frees them
    /// after the function call returns and subsequent `.source` /
    /// `.flags` reads dereference dangling memory. We side-table an
    /// owned `String` copy at construction time; readers prefer this
    /// over `pattern_ptr` whenever an entry exists.
    ///
    /// The copies are `Arc<str>` shared with `regex::site_cache`: every
    /// header built from the same literal text bumps two refcounts instead
    /// of copying the pattern (12 KB for emoji-class patterns, once per
    /// evaluation of the literal).
    static REGEX_SOURCE_TABLE: RefCell<crate::fast_hash::PtrHashMap<usize, (Arc<str>, Arc<str>)>> = RefCell::new(crate::fast_hash::new_ptr_hash_map());
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
    // RegExp is a GC-tracked `GC_TYPE_REGEXP` allocation, so it always carries
    // a preceding GcHeader; only read the magic field when the GC header says
    // this is an object of sufficient size to actually contain it.
    if regex_header_has_magic(ptr as *const RegExpHeader) {
        return true;
    }
    regex_pointers_contains(ptr as usize)
}

/// Monotone "this process has ever constructed a `RegExp`" latch.
///
/// The three `REGEX_POINTERS` probes all reach the thread-local table only
/// *after* the header-magic check misses — which is the common case, since they
/// are asked about ordinary objects on the generic property-dispatch path
/// (`object::exotic_expando::exotic_expando_kind`) and from `String.prototype`
/// dispatch. A program with no regex answers from one atomic load.
/// See `crate::registry_latch` for the ordering rule.
static REGEX_EVER_REGISTERED: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

#[inline]
fn regex_pointers_contains(addr: usize) -> bool {
    if REGEX_EVER_REGISTERED.is_idle() {
        return false;
    }
    REGEX_POINTERS.with(|s| s.borrow().contains(&addr))
}

/// Rekey every address-owned RegExp table after payload evacuation. Header
/// child slots are rewritten separately by the RegExp GC descriptor; this
/// hook handles the owner keys that a slot visitor cannot see.
pub(crate) fn regex_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == new_addr {
        return;
    }
    REGEX_POINTERS.with(|table| {
        let mut table = table.borrow_mut();
        if table.remove(&old_addr) {
            table.insert(new_addr);
        }
    });
    REGEX_SOURCE_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if let Some(source) = table.remove(&old_addr) {
            table.insert(new_addr, source);
        }
    });
    crate::object::exotic_expando::exotic_expando_owner_moved(old_addr, new_addr);
}

/// Remove address-owned RegExp metadata when the cell is proven dead.
pub(crate) fn regex_header_clear_dead_for_gc(addr: usize) {
    REGEX_POINTERS.with(|table| {
        table.borrow_mut().remove(&addr);
    });
    REGEX_SOURCE_TABLE.with(|table| {
        table.borrow_mut().remove(&addr);
    });
    crate::object::exotic_expando::exotic_expando_owner_clear_dead(addr);
}

/// Release the compiled programs owned by a dead `RegExpHeader`, then remove
/// its address-owned metadata.
///
/// The program pointers are raw `Arc` references installed by
/// `lazy::build_and_install_programs` or `RegExp.prototype.compile`. Null them
/// before reconstructing the `Arc`s because arena cleanup can visit the
/// metadata and finalizer paths for the same dead cell.
pub(crate) unsafe fn regex_header_finalize_for_gc(re: *mut RegExpHeader) {
    if re.is_null() {
        return;
    }
    #[cfg(feature = "regex-engine")]
    {
        let regex_ptr = (*re).regex_ptr;
        let fancy_ptr = (*re).fancy_ptr;
        let repeat_matcher_ptr = (*re).repeat_matcher_ptr;
        (*re).regex_ptr = ptr::null_mut();
        (*re).fancy_ptr = ptr::null();
        (*re).repeat_matcher_ptr = ptr::null();

        if !regex_ptr.is_null() {
            drop(Arc::from_raw(regex_ptr as *const Regex));
        }
        if !fancy_ptr.is_null() {
            drop(Arc::from_raw(fancy_ptr as *const fancy_regex::Regex));
        }
        if !repeat_matcher_ptr.is_null() {
            drop(Arc::from_raw(
                repeat_matcher_ptr as *const repeat_matcher::RepeatMatcherRegex,
            ));
        }
    }
    regex_header_clear_dead_for_gc(re as usize);
}

/// Finalize the RegExp headers that died in from-space during a copied minor.
///
/// The copying minor's from-space flip runs no per-object finalize hooks, so
/// a nursery header that was neither evacuated nor pinned would otherwise keep
/// its `Arc` programs and its `REGEX_POINTERS` / `REGEX_SOURCE_TABLE` / expando
/// entries forever. Same shape as `map::finalize_dead_copied_minor_from_space_maps`:
/// walk the registry after the flip, collect the provably-dead addresses, then
/// finalize each (the finalizer removes its own registry entries, which is why
/// the walk and the removal are two passes).
///
/// Cost: O(registry) = O(live headers + headers allocated since the last
/// minor) — the same order as the malloc sweep this replaces, and
/// proportional to allocation, not to program history.
pub(crate) fn finalize_dead_copied_minor_from_space_regexps() -> usize {
    let dead: Vec<usize> = REGEX_POINTERS.with(|table| {
        table
            .borrow()
            .iter()
            .copied()
            .filter(|&addr| {
                crate::gc::owner_is_dead_copied_minor_from_space_of_type(
                    addr,
                    crate::gc::GC_TYPE_REGEXP,
                )
            })
            .collect()
    });
    let count = dead.len();
    for addr in dead {
        unsafe { regex_header_finalize_for_gc(addr as *mut RegExpHeader) };
    }
    count
}

/// Sweep-entry twin of the above for the non-copying cycle kinds (fallback
/// minor / full mark-sweep): a dead header in the ACTIVE nursery allocation
/// block is never object-walked by any sweeper, so it is collected from the
/// registry right after trace instead (#6010, mirroring Map/Set/Buffer).
/// Deadness: unmarked ∧ not pinned ∧ not forwarded, and for a minor trace also
/// not tenured and physically in the nursery.
pub(crate) fn collect_dead_registered_regexps_post_trace(full_trace: bool) -> Vec<usize> {
    REGEX_POINTERS.with(|table| {
        table
            .borrow()
            .iter()
            .copied()
            .filter(|&addr| unsafe { registered_regexp_is_dead_post_trace(addr, full_trace) })
            .collect()
    })
}

/// Finalize one collected-dead RegExp (budget-chunked by the sweep state).
pub(crate) fn finalize_collected_dead_regexp(addr: usize) {
    unsafe { regex_header_finalize_for_gc(addr as *mut RegExpHeader) };
}

unsafe fn registered_regexp_is_dead_post_trace(addr: usize, full_trace: bool) -> bool {
    let Some(header) = crate::value::addr_class::try_read_gc_header(addr) else {
        return false;
    };
    if header.obj_type != crate::gc::GC_TYPE_REGEXP {
        return false;
    }
    let flags = header.gc_flags;
    if flags
        & (crate::gc::GC_FLAG_MARKED | crate::gc::GC_FLAG_PINNED | crate::gc::GC_FLAG_FORWARDED)
        != 0
    {
        return false;
    }
    if full_trace {
        return true;
    }
    if flags & crate::gc::GC_FLAG_TENURED != 0 {
        return false;
    }
    matches!(
        crate::arena::classify_heap_generation(addr),
        crate::arena::HeapGeneration::Nursery
    )
}

/// Test support: construct a RegExp through the PRODUCTION path
/// (`js_regexp_new`), run one `test()` so the compiled programs are installed
/// on the header, and hand the header back unrooted.
#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_construct_regexp_and_exec_once(pattern: &str, flags: &str) -> *mut RegExpHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let p = scope.root_string_ptr(js_string_from_str(pattern));
    let f = scope.root_string_ptr(js_string_from_str(flags));
    let re = p.with_mut_ptr::<StringHeader, _>(|p| {
        f.with_mut_ptr::<StringHeader, _>(|f| js_regexp_new(p, f))
    });
    let subject = scope.root_string_ptr(js_string_from_str("abc"));
    subject.with_const_ptr::<StringHeader, _>(|s| {
        let _ = js_regexp_test(re, s);
    });
    re
}

/// Test support: strong count of the standard program a header holds (the
/// observer clone taken here is released before returning).
#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_regexp_std_program_strong_count(re: *const RegExpHeader) -> usize {
    unsafe {
        let raw = (*re).regex_ptr as *const Regex;
        assert!(!raw.is_null(), "program must be installed");
        let arc = Arc::from_raw(raw);
        let n = Arc::strong_count(&arc);
        std::mem::forget(arc);
        n
    }
}

#[cfg(test)]
pub(crate) fn test_regex_pointer_entry_exists(addr: usize) -> bool {
    REGEX_POINTERS.with(|table| table.borrow().contains(&addr))
}

#[cfg(test)]
pub(crate) fn test_regex_source_entry_exists(addr: usize) -> bool {
    REGEX_SOURCE_TABLE.with(|table| table.borrow().contains_key(&addr))
}

/// Build a minimal nursery-resident RegExp payload for the copying collector's
/// relocation contract test. Production construction currently chooses the
/// malloc-backed arm of `ArenaOrMalloc`; this exercises the same registered GC
/// type through its arena arm so future allocator routing cannot silently
/// strand the address-owned tables.
#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_alloc_nursery_regexp_for_move(source: &str, flags: &str) -> *mut RegExpHeader {
    unsafe {
        let ptr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<RegExpHeader>(),
            std::mem::align_of::<RegExpHeader>(),
            crate::gc::GC_TYPE_REGEXP,
        ) as *mut RegExpHeader;
        // Neither `gc_malloc` nor the arena zeroes reused memory, so this
        // must be set explicitly or the GC follows a garbage pointer.
        (*ptr).meta = std::ptr::null_mut();
        (*ptr).regex_ptr = std::ptr::null_mut();
        (*ptr).pattern_ptr = std::ptr::null();
        (*ptr).flags_ptr = std::ptr::null();
        (*ptr).case_insensitive = flags.contains('i');
        (*ptr).global = flags.contains('g');
        (*ptr).multiline = flags.contains('m');
        (*ptr).sticky = flags.contains('y');
        (*ptr).dot_all = flags.contains('s');
        (*ptr).unicode = flags.contains('u') || flags.contains('v');
        (*ptr).has_indices = flags.contains('d');
        (*ptr).last_index = crate::value::JSValue::number(0.0).bits();
        (*ptr).magic = REGEXP_MAGIC;
        (*ptr).fancy_ptr = std::ptr::null();
        (*ptr).repeat_matcher_ptr = std::ptr::null();

        REGEX_EVER_REGISTERED.arm();
        REGEX_POINTERS.with(|table| {
            table.borrow_mut().insert(ptr as usize);
        });
        REGEX_SOURCE_TABLE.with(|table| {
            table
                .borrow_mut()
                .insert(ptr as usize, (Arc::from(source), Arc::from(flags)));
        });
        ptr
    }
}

/// Bounds-checked read of `RegExpHeader.magic`. Confirms the preceding
/// `GcHeader` exists, is a `GC_TYPE_REGEXP`, and the allocation is large enough
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
        if gc.obj_type != crate::gc::GC_TYPE_REGEXP {
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
/// The compiled matcher pointers point to OFF-heap leaked Rust allocations and the
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
crate::perry_thread_local! {
    /// Cache of compiled regex objects, keyed by (pattern, flags).
    static REGEX_CACHE: RefCell<HashMap<ProgramKey, Arc<Regex>>> = RefCell::new(HashMap::new());
    /// Fancy-regex fallback cache for patterns with lookbehind/lookahead.
    static FANCY_CACHE: RefCell<HashMap<ProgramKey, Arc<fancy_regex::Regex>>> = RefCell::new(HashMap::new());

    /// ECMAScript backtracking matchers for quantified capture groups. These
    /// are the patterns where `regex`/`fancy-regex` cannot reproduce
    /// `RepeatMatcher` capture reset and nullable-iteration semantics (#5897).
    static REPEAT_MATCHER_CACHE: RefCell<HashMap<ProgramKey, Arc<repeat_matcher::RepeatMatcherRegex>>> = RefCell::new(HashMap::new());

    /// `(pattern, flags)` pairs that have already cleared construction-time
    /// validation. Validity is a pure function of the pair, so the answer is
    /// worth remembering; `js_regexp_new` used to get this from a
    /// `REGEX_CACHE` hit, which stopped being a proxy once the compiled
    /// program became lazy (see `regex::lazy`). Same cap and
    /// clear-on-overflow policy as the program caches — the cost of a clear
    /// is a repeated parse, never a wrong verdict. The unit value keeps
    /// `evict_regex_cache_if_full` shared with the three program caches.
    static VALIDATED_PATTERNS: RefCell<HashMap<(String, String), ()>> = RefCell::new(HashMap::new());
}

#[cfg(feature = "regex-engine")]
mod compile_cache;
#[cfg(feature = "regex-engine")]
pub(crate) use compile_cache::*;

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
    /// Header-owned `Arc<RepeatMatcherRegex>` for quantified capture groups,
    /// or null for the ordinary linear/fancy paths. Like `fancy_ptr`, this
    /// survives cache eviction and duplicate statically-linked runtime copies.
    pub repeat_matcher_ptr: *const (),
    /// #6759 phase 1 (header unification): per-object metadata record, or
    /// null. Appended LAST so `regex_gc_slot_ptrs`' adjacency assertion on
    /// `pattern_ptr`/`flags_ptr` and every other offset are undisturbed.
    ///
    /// RegExp's rewrite descriptor DELEGATES to the layout visitor, so unlike
    /// Error/Map/Set the edge belongs in `gc_child_slots`
    /// (`GcLayoutSlotKind::RegExpFields`) — that is the marking path here.
    /// #6812 is precisely the bug of putting it in the wrong one.
    pub meta: *mut crate::object::ObjectMeta,
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
    regex_pointers_contains(p as usize)
}

/// Public: is `addr` a RegExpHeader we allocated via `js_regexp_new`?
/// Used by the console/`util.inspect` formatter to print regex literals
/// as `/source/flags` instead of `{}` (they're GC_TYPE_REGEXP allocations
/// with no enumerable string keys). Registry-gated so a generic object
/// is never mis-read as a RegExpHeader.
pub fn is_registered_regex(addr: usize) -> bool {
    // Wall 18: header magic first (duplicate-runtime thread-local resilient).
    if regex_header_has_magic(addr as *const RegExpHeader) {
        return true;
    }
    regex_pointers_contains(addr)
}

/// Internal helper: Get string data from StringHeader
pub(crate) fn string_as_str<'a>(s: *const StringHeader) -> &'a str {
    unsafe { std::str::from_utf8_unchecked(string_as_bytes(s)) }
}

/// Internal helper: get the byte payload without assuming it is Unicode
/// scalar UTF-8. JavaScript strings containing lone surrogates use WTF-8.
pub(crate) fn string_as_bytes<'a>(s: *const StringHeader) -> &'a [u8] {
    unsafe {
        let len = (*s).byte_len as usize;
        let data = (s as *const u8).add(std::mem::size_of::<StringHeader>());
        std::slice::from_raw_parts(data, len)
    }
}

/// Internal helper: Create a StringHeader from a Rust &str
pub(super) fn js_string_from_str(s: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

#[cfg(feature = "regex-engine")]
/// Throw a `SyntaxError` with the given message and never return.
#[cfg(feature = "regex-engine")]
pub(super) fn throw_regexp_syntax_error(message: &str) -> ! {
    let msg = js_string_from_str(message);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

/// Kill switch for the newborn-parent barrier gate below
/// (`PERRY_REGEX_NEWBORN_BARRIER_GATE=0` ⇒ the two header stores take the
/// unconditional barrier pair, i.e. the pre-gate code path exactly). One
/// relaxed load of a `OnceLock` per construction, resolved once per process,
/// mirroring `regex::site_cache::enabled`.
#[cfg(feature = "regex-engine")]
#[inline]
fn newborn_barrier_gate_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        crate::gc::env_default_on_from_value(
            std::env::var("PERRY_REGEX_NEWBORN_BARRIER_GATE")
                .ok()
                .as_deref(),
        )
    })
}

/// Create a new RegExp from pattern and flags strings
/// Returns a pointer to RegExpHeader
///
/// Validates the pattern and allocates the header; it does NOT build the
/// compiled program. That happens on the first operation that needs a matcher
/// — see `regex::lazy`, and the `regex_ptr`/`fancy_ptr`/`repeat_matcher_ptr`
/// fields, which are null until then. A fresh header per call is required:
/// ECMA-262 evaluates a regex literal to a NEW object every time, and the
/// distinction is observable through `===`, expandos and `lastIndex`.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_new(
    pattern: *const StringHeader,
    flags: *const StringHeader,
) -> *mut RegExpHeader {
    js_regexp_new_impl(pattern, flags, 0)
}

/// [`js_regexp_new`] for a **regex literal**, which the compiler can identify
/// by its source site instead of by its text.
///
/// `site_key` is the address of an 8-byte private global the `Expr::RegExp`
/// lowering emits once per literal (`expr/logical_collections.rs`). It is
/// unique by construction, immortal, and never moves, which is what makes it a
/// sound identity where a `StringHeader` address is not: string headers are
/// GC-managed, so an address is freed and reused and a moving collector
/// relocates them, and a pointer-keyed cache over them would answer for a
/// different pattern.
///
/// A hit therefore verifies with ONE word compare (plus the site's ≤ 8-byte
/// flags text) and never reads the pattern at all — no fingerprint, no
/// `memcmp`, no validation, no flag canonicalization. On claude-code the
/// segment loop constructs `string-width`'s ~12,807-character `/…/g` once per
/// grapheme, and the content cache's exactness verify alone is ~2.0 GB of
/// `memcmp` per 400-character reply.
///
/// A `site_key` of 0 means "no site" and behaves exactly like
/// [`js_regexp_new`]; every dynamic construction (`new RegExp(s)`,
/// [`js_regexp_construct`], the runtime's own callers) keeps the two-argument
/// form and never touches the site table.
///
/// Kill switch: `PERRY_REGEX_SITE_KEY=0`.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_new_site(
    pattern: *const StringHeader,
    flags: *const StringHeader,
    site_key: i64,
) -> *mut RegExpHeader {
    js_regexp_new_impl(pattern, flags, site_key as usize)
}

#[cfg(feature = "regex-engine")]
fn js_regexp_new_impl(
    pattern: *const StringHeader,
    flags: *const StringHeader,
    site_key: usize,
) -> *mut RegExpHeader {
    // ★ `pattern` is a raw `StringHeader*` in a Rust local, and this function
    // allocates twice below (`js_string_from_str` for the canonical flags, then
    // `arena_alloc_gc` for the header). Either can drive an evacuating minor that
    // relocates the pattern string, after which this argument names retired
    // from-space — and it is then *stored into the header* as `pattern_ptr`,
    // so the damage is permanent rather than transient.
    //
    // This is the runtime-Rust half of the rooting invariant (#7249), the half
    // `scripts/gc_root_dominance_check.py` is structurally blind to: it reads
    // emitted IR and cannot see a Rust local. It was found the way that class
    // has to be found — `PERRY_GC_PROTECT_FROMSPACE=1
    // PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` faulted here on a `zod` workload,
    // in `js_regexp_new` itself, on BOTH sides of an unrelated codegen change.
    let scope = crate::gc::RuntimeHandleScope::new();
    let pattern_root = scope.root_string_ptr(pattern);
    let raw_flags_str = if is_valid_ptr(flags) {
        string_as_str(flags)
    } else {
        ""
    };

    // ★ LITERAL-SITE FAST PATH — identity by an immortal address.
    //
    // `site_key` is the address of a private global the compiler emits once
    // per regex literal, so a match on it proves this is the SAME SOURCE SITE
    // that recorded the entry, whose pattern and flags are fixed at compile
    // time. Nothing about the pattern text is read: no fingerprint, no
    // `memcmp`, no validation, no flag canonicalization. The flags text IS
    // compared, because it is at most eight bytes and because two spellings of
    // one canonical form (`/x/ig`, `/x/gi`) must not answer for each other.
    //
    // A `site_key` of 0 (every dynamic construction, and every runtime caller)
    // misses by construction and takes the content-keyed path below unchanged.
    let site_entry = site_key::lookup(site_key, raw_flags_str);
    let (owned_pattern, owned_flags, programs, bits, shared_flags_root) = match site_entry {
        Some(hit) => {
            // The site's own flags literal, so this is the same sharing
            // decision the first construction at this site made (#9819).
            let shared_flags_root = (hit.flags_are_canonical && is_valid_ptr(flags))
                .then(|| scope.root_string_ptr(flags));
            debug_assert!(
                !is_valid_ptr(pattern) || string_as_str(pattern) == &*hit.pattern,
                "a site key names ONE source literal, whose pattern text cannot change; a \
                 caller that reuses a key for different text would silently take another \
                 site's program"
            );
            if crate::hot_diag::regex_on() {
                let bytes: &[u8] = hit.pattern.as_bytes();
                let flags_text: &str = &hit.flags;
                crate::hot_diag::regex_with(|d| {
                    d.new_site_key_hit += 1;
                    d.note_new(pattern as usize, bytes, flags_text, false, true);
                });
            }
            // Until the site's first execution installs the compiled programs,
            // pick them up from the content cache — one probe per construction,
            // and in a loop that matches immediately that is exactly one.
            let programs = match hit.programs {
                Some(programs) => Some(programs),
                None => {
                    let picked =
                        site_cache::lookup(&hit.pattern, &hit.flags).and_then(|h| h.programs);
                    if let Some(programs) = picked.clone() {
                        site_key::install_programs(site_key, programs);
                    }
                    picked
                }
            };
            (
                hit.pattern,
                hit.flags,
                programs,
                hit.bits,
                shared_flags_root,
            )
        }
        None => {
            let pattern_str = if is_valid_ptr(pattern) {
                string_as_str(pattern)
            } else {
                ""
            };

            // #2829: reject duplicate/unknown flags (SyntaxError) and store the
            // canonical sorted form so `.flags` reflects Node's ordering.
            let canonical_flags = validate_and_canonicalize_flags(raw_flags_str);
            let flags_str = canonical_flags.as_str();

            // ★ Share the caller's flags string when it is ALREADY the canonical text.
            //
            // `flags_ptr` used to be a fresh `js_string_from_str` on every
            // construction. A JS regex literal evaluates to a fresh RegExp object
            // every time it is reached, so that is one 32-byte GC string per
            // evaluation: `PERRY_REGEX_DIAG` counts 161,897 constructions per
            // 400-character claude-code reply, ~5.2 MB of identical one- and two-byte
            // strings, and ~44 MB on a 3300-character reply.
            //
            // JS strings are immutable and have no identity semantics, and a literal's
            // flags text is written by the author in spec order (`/x/gi`, not
            // `/x/ig`), so the caller's string usually IS the canonical text and can
            // simply be shared. Nothing downstream depends on the pointer being fresh:
            // `flags_ptr`-keyed lookups (`FANCY_CACHE`, `lookup_fancy_regex`) read it
            // through `string_as_str` and compare CONTENT, and the header keeping a
            // pointer to it is what keeps it alive.
            //
            // This comparison must happen HERE, before the validation block below,
            // because `raw_flags_str` borrows the caller's GC string and that block
            // can allocate. The root is taken here for the same reason: the raw
            // `flags` argument may name from-space after any allocation, exactly as
            // the ★ note on `pattern_root` says, and this one is stored into the
            // header too.
            let flags_are_canonical = raw_flags_str == flags_str;
            let shared_flags_root =
                (is_valid_ptr(flags) && flags_are_canonical).then(|| scope.root_string_ptr(flags));
            // Materialized HERE, while `raw_flags_str`'s borrow of the caller's
            // GC string is still guaranteed live: the validation block below
            // can allocate, and the site record is written after it.
            let raw_flags_owned: Arc<str> = Arc::from(raw_flags_str);

            let case_insensitive = flags_str.contains('i');
            let global = flags_str.contains('g');
            let multiline = flags_str.contains('m');
            let sticky = flags_str.contains('y');
            let dot_all = flags_str.contains('s');
            let unicode = flags_str.contains('u') || flags_str.contains('v');
            let has_indices = flags_str.contains('d');

            // Content-keyed construction cache (`regex::site_cache`): a verified hit
            // means this exact `(pattern, canonical flags)` already cleared the
            // validation below — validity is a pure function of the pair — and hands
            // back the shared owned copies plus, once some header built from this
            // text has been executed, its compiled programs. The probe is one
            // fingerprint and one byte compare; everything below it that copies or
            // hashes the pattern is skipped.
            let site_hit = site_cache::lookup(pattern_str, flags_str);
            let validated_hit =
                site_hit.is_some() || lazy::pattern_already_validated(pattern_str, flags_str);
            if crate::hot_diag::regex_on() {
                crate::hot_diag::regex_with(|d| {
                    d.note_new(
                        pattern as usize,
                        pattern_str.as_bytes(),
                        flags_str,
                        validated_hit && site_hit.is_none(),
                        site_hit.is_some(),
                    )
                });
            }

            // #2829: reject invalid pattern syntax with a SyntaxError. A pattern the
            // `regex` crate rejects is only a real error if `fancy-regex` (which
            // covers the full JS feature set: lookbehind/lookahead/backreferences)
            // ALSO rejects it — otherwise it is a valid JS pattern we route through
            // the fancy fallback. `get_or_compile_regex` populates FANCY_CACHE when
            // the regex crate fails but fancy-regex succeeds; check both here.
            //
            // PERF (#5777 follow-up): the ENTIRE validation block runs at most once
            // per (pattern, flags). Regex validity is a pure function of the pair, so
            // a pattern that has already cleared it can never fail it later; the
            // cheap JS-syntax checks are not actually cheap
            // (`has_invalid_repeated_quantifier` does a
            // `pattern.chars().collect::<Vec<char>>()` — a ~51 KB allocation for a
            // 12,807-char pattern — plus an O(n) scan on EVERY `new RegExp(...)`),
            // and the common `string-width`/`emoji-regex` npm packages construct a
            // fresh ~12,807-char `/…/g` literal on every measurement, which a layout
            // pass calls thousands of times. #5777 keyed that skip off a REGEX_CACHE
            // hit, which worked only because construction also COMPILED; with the
            // build deferred, the fact is recorded directly in `VALIDATED_PATTERNS`.
            {
                if !validated_hit {
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
                    if !flags_str.contains('v')
                        && has_out_of_order_double_dash_class_range(pattern_str)
                    {
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
                    // The remaining question — "is this a SyntaxError?" — used to be
                    // answered by BUILDING the pattern, which is why constructing a
                    // regex cost an NFA. Ask the standard engine's PARSER instead
                    // (`lazy::std_engine_syntax_ok`, the same `regex_syntax` parse
                    // `build_std_regex` performs, on the same string): 17.8x cheaper,
                    // and it agrees with the full build on every one of the 2,378
                    // regex literals in the claude-code bundle (asserted over a
                    // corpus by `tests::syntax_check_agrees_with_full_build`).
                    //
                    // A parser rejection is NOT a verdict: every lookbehind /
                    // backreference pattern is rejected by the linear engine too. Fall
                    // through to the unchanged both-engines path, which owns the
                    // SyntaxError decision and populates the caches for the fancy
                    // fallback.
                    if !lazy::std_engine_syntax_ok(pattern_str, flags_str)
                // Cold: the linear engine's parser refused, so only a BUILD
                // can tell a fancy-regex pattern from a SyntaxError.
                // Materialising the `Arc` key happens once per distinct
                // pattern that needs the fallback, not per object.
                && !compile_and_cache_regex_checked(
                    &Arc::from(pattern_str),
                    &Arc::from(flags_str),
                ) {
                        // Preserve the historical edge: validation used to test the
                        // BARE translated pattern (no `(?ims)` prefix). A pattern that
                        // compiles bare but blows the size limit with the flag prefix
                        // must stay a silent never-match (matching prior behavior),
                        // not a SyntaxError.
                        let translated = js_regex_to_rust(pattern_str);
                        if build_std_regex(&translated).is_err()
                            && build_fancy_regex(&translated).is_err()
                        {
                            throw_regexp_syntax_error(&format!(
                                "Invalid regular expression: /{}/: invalid pattern",
                                pattern_str
                            ));
                        }
                    }
                    lazy::mark_pattern_validated(pattern_str, flags_str);
                }
            }

            // The compiled program is NOT built here. Validation above has already
            // established that the pattern is legal, and a bundle evaluates hundreds
            // of module-level literals it never matches with — building each one's
            // NFA at construction is what put ~14% of a claude-code `--help` run
            // inside `regex_syntax`/`regex_automata`. `regex_ptr` stays null (the
            // "not built yet" state) and `lazy::ensure_regex_compiled` installs the
            // owned `Arc`s on the first operation that needs a matcher.

            // ★ Last use of the borrowed pattern text before this function allocates.
            // `pattern_str` borrows the GC string; the two allocations below can move
            // it, and everything after this point reads the pattern from `owned_pattern`
            // (a shared `Arc<str>`, which relocation cannot invalidate) or from
            // `pattern_root` (a runtime handle the collector rewrites). Nothing below
            // may use `pattern_str` or the incoming `pattern` argument again.
            let (owned_pattern, owned_flags, programs) = match site_hit {
                Some(hit) => (hit.pattern, hit.flags, hit.programs),
                None => {
                    let (p, f) = site_cache::insert(pattern_str, flags_str);
                    (p, f, None)
                }
            };
            #[allow(unused_variables)]
            let pattern_str: () = ();

            // Record what this construction established, so every later
            // evaluation of this literal answers from the site key. Only ever
            // written on the path that has already validated the pair — a
            // hit legitimately skips validation because validity is a pure
            // function of `(pattern, flags)`.
            let bits = site_key::FlagBits {
                case_insensitive,
                global,
                multiline,
                sticky,
                dot_all,
                unicode,
                has_indices,
            };
            site_key::record(
                site_key,
                raw_flags_owned,
                owned_pattern.clone(),
                owned_flags.clone(),
                flags_are_canonical,
                bits,
                programs.clone(),
            );
            (
                owned_pattern,
                owned_flags,
                programs,
                bits,
                shared_flags_root,
            )
        }
    };
    let site_key::FlagBits {
        case_insensitive,
        global,
        multiline,
        sticky,
        dot_all,
        unicode,
        has_indices,
    } = bits;

    // ★ The header is NURSERY-allocated, like an ordinary object.
    //
    // It used to be `gc_malloc`'d: raw `alloc()` at first (a 64-byte leak per
    // construction), then the tracked malloc arm so the sweep could free it.
    // That arm costs, PER CONSTRUCTION, a mimalloc allocation, a push onto
    // `MALLOC_STATE.objects`, an insert into the malloc-registry `PtrHashSet`
    // (which rehashes as it grows), two old→young remembered-set entries for
    // `pattern_ptr`/`flags_ptr`, and — at death — a malloc-sweep visit, the
    // finalizer and a free. A JS regex literal constructs a fresh object every
    // time it is evaluated, so on the claude-code TUI `PERRY_GC_TRACE` counted
    // **199,873 RegExp headers malloc'd per 400-character reply — 100.0 % of
    // all malloc allocations** — with the registry swinging 26,690 → 1,689
    // across one minor: ~94 % of them die young and were paying
    // old-generation prices to do it.
    //
    // `GC_TYPE_REGEXP` has been movable (`GcMoveHookKind::RegExpSideTables`
    // rekeys `REGEX_POINTERS`, `REGEX_SOURCE_TABLE` and the expando owner
    // after evacuation; `GcLayoutSlotKind::RegExpFields` traces the two string
    // edges and `meta`) since the copying collector landed, and
    // `test_movable_regexp_evacuation_migrates_all_address_owned_state` has
    // exercised the arena arm all along. What kept production on malloc was
    // finalization: the copying minor's from-space flip runs no per-object
    // finalize hooks (`gc::copying`), so a nursery header that dies young
    // would leak its three `Arc` programs and its registry entries. That is
    // now handled the way Map/Set/Error handle theirs —
    // `finalize_dead_copied_minor_from_space_regexps` after a copied minor and
    // `collect_dead_registered_regexps_post_trace` at sweep entry for the
    // non-copying cycle kinds — and a tenured header is finalized by the
    // old-generation sweep's ordinary `gc_type_finalize_unmarked_payload`.
    let header_size = std::mem::size_of::<RegExpHeader>();
    // `flags_ptr` must hold the CANONICAL form, so that `flags_ptr`-keyed
    // lookups (FANCY_CACHE, lookup_fancy_regex) and the GC-survivable source
    // table all agree. When the caller's string already is that text it is
    // shared (rooted above); only a non-canonical spelling (`/x/ig` → `"gi"`,
    // or a computed `new RegExp(p, f)`) still has to materialize one. The
    // counter makes the removal provable rather than asserted.
    let flags_root = match shared_flags_root {
        Some(root) => root,
        None => {
            if crate::hot_diag::regex_on() {
                crate::hot_diag::regex_with(|d| d.new_flags_allocated += 1);
            }
            // `owned_flags` IS the canonical text (the shared `Arc<str>` the
            // site or content cache handed back), and unlike `flags_str` it
            // does not borrow the caller's GC string, so it is still valid
            // here after the analysis above.
            scope.root_string_ptr(js_string_from_str(&owned_flags))
        }
    };
    // ★ #7341: root the canonical flags string too. The header allocation below
    // is an allocation and therefore a collection point, exactly as the comment
    // above `pattern_root` says — but only the PATTERN was rooted and re-read.
    // The flags string is created here and stored into the header AFTER that
    // allocation, so an evacuating minor in the header allocation moved it and
    // the header kept the pre-collection address. `flags_ptr` is then permanently stale in
    // a live header: `lookup_fancy_regex` reads it through `string_as_str` and
    // faults on retired from-space, which is 5 of the 31 catches in #7341
    // (four different callers, all reaching that one read).
    //
    // The write barrier below already treated this as a real GC edge; what was
    // missing is that the value written had to survive the allocation first.

    unsafe {
        let raw = crate::arena::arena_alloc_gc(
            header_size,
            std::mem::align_of::<RegExpHeader>(),
            crate::gc::GC_TYPE_REGEXP,
        );
        if raw.is_null() {
            // #5067 — catchable RangeError instead of aborting on OOM.
            crate::error::throw_allocation_failed();
        }
        let ptr = raw as *mut RegExpHeader;
        // A previous (collected) RegExp at this address may have left expando
        // properties in the side table; a fresh RegExp must start clean.
        crate::object::exotic_expando::expando_clear_on_alloc(ptr as usize);

        // ★ Re-read the pattern from its root. Both allocations above have run,
        // so the incoming argument may name from-space; the handle is a mutable
        // root the collector rewrote.
        let pattern = pattern_root.get_raw_const_ptr::<StringHeader>();
        // #7341: same re-read for the flags, for the same reason.
        let canonical_flags_ptr = flags_root.get_raw_const_ptr::<StringHeader>();

        // Neither `gc_malloc` nor the arena zeroes reused memory, so this
        // must be set explicitly or the GC follows a garbage pointer.
        (*ptr).meta = std::ptr::null_mut();
        // Null = not compiled yet; see `lazy::ensure_regex_compiled`.
        (*ptr).regex_ptr = std::ptr::null_mut();
        (*ptr).pattern_ptr = pattern;
        (*ptr).flags_ptr = canonical_flags_ptr;
        // `pattern_ptr` / `flags_ptr` are GC-managed StringHeaders — the GC scans
        // this 2-slot payload range via the magic-tagged RegExp layout.
        //
        // The header is young now, so for a young child the barrier records
        // nothing; it still has to run, because a header born while a budgeted
        // cycle is marking is allocated black, and because a header that has
        // been promoted and then reassigned (`RegExp.prototype.compile`) is a
        // genuine old→young store. Historically the header was malloc'd, i.e.
        // old, and this store was THE old→young edge a copying minor would
        // otherwise miss: the evacuation verifier reported it as an uncovered
        // object→string edge, and it crashed for real when the freed slot was
        // later scanned/read (a heavy regex workload — e.g. ANSI/emoji parsing
        // in a terminal UI — hit it within seconds).
        //
        // Remember both edges, mirroring every other native-header pointer
        // store (closure captures, object prototype slots, array headers).
        // `runtime_write_barrier_gc_slot` classifies the parent and only
        // remembers genuinely-young children, so an already-old/interned
        // `pattern` is a harmless no-op.
        //
        // ★ Gated by the same live header test the COMPILER emits in front of
        // every one of its own stores (`emit_parent_may_need_remembering_check`,
        // #7511): a parent whose `GC_FLAG_TENURED` is clear owes the
        // remembered set nothing, and a globally idle incremental barrier
        // makes the SATB shading skippable too. Both clauses are read live —
        // a header a collection promoted between `arena_alloc_gc` above and
        // this store reads TENURED here and takes the full path, as does
        // `RegExp.prototype.compile` reassigning a tenured header.
        //
        // Since #9845 the header is a NURSERY allocation, so on the common
        // path both clauses are false and the pair of barrier calls — four
        // page-map classifications, two dirty-page-cache probes and two child
        // classifications, all ending at `ParentNotOldSkips` — collapses to
        // one relaxed load of a static and one byte read of the header this
        // function just wrote. `PERRY_REGEX_NEWBORN_BARRIER_GATE=0` restores
        // the unconditional pair; nothing else changes with the gate off, so
        // the OFF arm is the pre-change code path exactly.
        let regexp_parent_addr = ptr as usize;
        let needs_barrier = !newborn_barrier_gate_enabled()
            || crate::gc::newborn_parent_needs_barrier(regexp_parent_addr);
        if crate::hot_diag::regex_on() {
            crate::hot_diag::regex_counters(|d| {
                if needs_barrier {
                    d.new_barrier_taken += 1;
                } else {
                    d.new_barrier_gated += 1;
                }
                d.new_header_bytes += header_size as u64;
            });
        }
        if needs_barrier {
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
        // The header-resident fancy-regex fallback (lookahead/lookbehind/
        // backrefs) and the ECMAScript backtracking matcher are installed
        // alongside `regex_ptr` by `lazy::ensure_regex_compiled`, from the
        // same caches, on the first operation that needs a matcher. Keeping
        // all three on one publish point is what makes `regex_ptr.is_null()`
        // a sound built/not-built flag.
        (*ptr).fancy_ptr = std::ptr::null();
        (*ptr).repeat_matcher_ptr = std::ptr::null();
        // Born built: the site cache already holds the programs the first
        // execution of this text compiled. Install the same three owned
        // references `lazy::build_and_install_programs` would, publishing
        // `regex_ptr` last for the same reason it does.
        if let Some(programs) = programs {
            (*ptr).fancy_ptr = programs
                .fancy
                .map_or(std::ptr::null(), |arc| Arc::into_raw(arc) as *const ());
            (*ptr).repeat_matcher_ptr = programs
                .repeat
                .map_or(std::ptr::null(), |arc| Arc::into_raw(arc) as *const ());
            (*ptr).regex_ptr = Arc::into_raw(programs.std) as *mut Regex;
        }

        // Record the pointer so that js_string_split can detect
        // `s.split(regex)` without a dedicated runtime decl.
        // Arm before the insert — see `crate::registry_latch`.
        REGEX_EVER_REGISTERED.arm();
        REGEX_POINTERS.with(|s| {
            s.borrow_mut().insert(ptr as usize);
        });
        if crate::hot_diag::regex_on() {
            // Two address-keyed inserts per construction (this one and
            // `REGEX_SOURCE_TABLE` below), each a `PtrHasher` hash plus a
            // hashbrown insert, mirrored by two removals at death and two
            // rekeys per evacuation. Counted so the pair is a number rather
            // than a reading of the profile.
            crate::hot_diag::regex_counters(|d| d.new_side_table_inserts += 2);
        }

        // Issue #637: side-table owned copies of pattern + flags so
        // `.source` / `.flags` survive GC of the input StringHeaders.
        REGEX_SOURCE_TABLE.with(|t| {
            t.borrow_mut()
                .insert(ptr as usize, (owned_pattern, owned_flags));
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
            Some((pat, fl)) => (pat.to_string(), Some(fl.to_string())),
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

/// `regex.test(haystack)` where `haystack` is a **bounded slice whose bounds
/// ARE the string's ends** — the primitive the `Intl.Segmenter` view mode needs
/// so a segment can be tested without being materialised.
///
/// Passing a sub-slice rather than a start offset is the whole point: `^`
/// anchors at the slice start, `$` at its end, and a lookbehind cannot see the
/// preceding grapheme, which is exactly what `test` on the materialised
/// substring means. A start-offset match would be silently wrong for an
/// anchored pattern, which is what claude-code's `oR_` is.
///
/// Returns `None` — "I decline, materialise and call the normal path" — for a
/// **global or sticky** regex, because `test` is then stateful: it must consult
/// and advance `lastIndex`, and that bookkeeping (`regexp_find_advancing`) is
/// written against a `StringHeader`, not a slice. Answering it from a slice
/// would either lose the update or invent one.
// Its body reaches `diag_note_op` and `exec::`, both engine-gated, and its only
// caller (`js_segments_view_regexp_test`) references it only under this feature.
#[cfg(feature = "regex-engine")]
pub(crate) fn regexp_test_str_bounded(re: *const RegExpHeader, hay: &str) -> Option<bool> {
    if !is_valid_regex_ptr(re) {
        return None;
    }
    unsafe {
        if (*re).global || (*re).sticky {
            return None;
        }
        if crate::hot_diag::regex_on() {
            diag_note_op(re, crate::hot_diag::RegexOp::Test);
        }
        if let Some(repeat_matcher) = lookup_repeat_matcher(re) {
            return Some(repeat_matcher.regex.find(hay).is_some());
        }
        if let Some(fre) = lookup_fancy_regex(re) {
            return match fre.is_match(hay) {
                Ok(v) => Some(v),
                Err(_) => None,
            };
        }
        Some(lazy::header_std_regex(re).is_match(hay))
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
        if crate::hot_diag::regex_on() {
            diag_note_op(re, crate::hot_diag::RegexOp::Test);
            if (*re).global || (*re).sticky {
                crate::hot_diag::regex_with(|d| d.test_global += 1);
            }
        }
        // For global/sticky regexes `test` is stateful — it must consult and
        // advance `lastIndex` (and anchor for sticky) exactly like `exec`. The
        // find-only twin of `exec`'s engine phase does that bookkeeping without
        // materializing a result array: `test` only reports whether a match
        // was produced, and building the captures array plus one string per
        // capture per call was the allocation `ansi-regex`-style `g` tests
        // paid on every text segment.
        if (*re).global || (*re).sticky {
            return if exec::regexp_find_advancing(re as *mut RegExpHeader, s).is_some() {
                1
            } else {
                0
            };
        }

        if let Some(repeat_matcher) = lookup_repeat_matcher_for(re, str_data, 0) {
            return if repeat_matcher.regex.find(str_data).is_some() {
                1
            } else {
                0
            };
        }

        if let Some(fre) = lookup_fancy_regex(re) {
            return match fre.is_match(str_data) {
                Ok(true) => 1,
                Ok(false) | Err(_) => 0,
            };
        }

        let regex = lazy::header_std_regex(re);
        if regex.is_match(str_data) {
            1
        } else {
            0
        }
    }
}

/// `PERRY_REGEX_DIAG`: attribute one exec-family operation to the receiver's
/// pattern. Callers have already validated `re`.
#[cfg(feature = "regex-engine")]
pub(super) fn diag_note_op(re: *const RegExpHeader, op: crate::hot_diag::RegexOp) {
    unsafe {
        let pattern_ptr = (*re).pattern_ptr;
        let flags_ptr = (*re).flags_ptr;
        let pattern = if is_valid_ptr(pattern_ptr) {
            string_as_bytes(pattern_ptr)
        } else {
            b""
        };
        let flags = if is_valid_ptr(flags_ptr) {
            string_as_str(flags_ptr)
        } else {
            ""
        };
        crate::hot_diag::regex_with(|d| d.note_op(pattern_ptr as usize, pattern, flags, op));
    }
}

/// Look up a fancy-regex fallback for the given header, if one was
/// registered at compile-time because the `regex` crate rejected the
/// pattern (backreferences, lookbehind, etc.).
#[cfg(feature = "regex-engine")]
pub(crate) fn lookup_fancy_regex(re: *const RegExpHeader) -> Option<Arc<fancy_regex::Regex>> {
    // The header's programs are built on first use; `fancy_ptr` is null until
    // then, and a null there is indistinguishable from "this pattern has no
    // fancy fallback" — so build before reading it.
    lazy::ensure_regex_compiled(re);
    unsafe {
        // Wall 18: header-resident fancy Arc first (duplicate-runtime
        // thread-local resilient). `fancy_ptr` is a leaked `Arc` raw pointer; to
        // hand back an owned `Arc` clone WITHOUT consuming the header's
        // reference, reconstruct, clone, then `mem::forget` the reconstructed
        // one so the header's strong count is preserved.
        if regex_header_has_magic(re) {
            if (*re).fancy_ptr.is_null() {
                // Built (see `ensure_regex_compiled` above) with no fancy
                // fallback: every install path (`lazy`, `compile`, the site
                // cache) publishes all three program pointers together, so a
                // null here is the answer, not "not looked up yet". Falling
                // through to the cache probe re-hashed the whole pattern on
                // EVERY exec of every ordinary regex (#keystroke profile:
                // 514 samples under this function alone).
                return None;
            }
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
                .get(&(Arc::from(pat), Arc::from(flags_str)))
                .cloned()
        })
    }
}

/// Can the linear program prove there is no match at or after `start`?
///
/// # The cliff this closes
///
/// `repeat_matcher::capture_layout` takes a pattern OFF the linear engine when
/// ECMA-262's RepeatMatcher capture semantics are observable — a capture group
/// directly under a quantifier, or a capture inside a negative lookaround. That
/// is a correctness requirement (the linear engine keeps the last value of a
/// capture nested in a quantified group; the spec clears it every iteration),
/// but it means adding parentheses moves a pattern onto `regress`, a classical
/// backtracker with no step budget. Measured: `/^(a+)+$/.test("a"*28 + "!")`
/// costs **16.5 s** while `/^(?:a+)+$/` — the same language, no capture — is
/// instant, and node does the same test in 4.8 s. **6.3 %** of the 4,463
/// distinct regex literals across seven real bundles take this route
/// (claude-code 7.1 %, dayjs 25 %, luxon 29 %).
///
/// The two engines accept exactly the same LANGUAGE for a pattern they both
/// compile; they disagree only about which capture assignment to report. So a
/// linear "there is no match here" is a real no, reachable in O(n) where the
/// backtracker's no can cost O(2^n). Ask it first, and the exponential case —
/// which is always a NON-matching subject, because that is what a ReDoS input
/// is — never reaches the backtracker.
///
/// Returns false (i.e. "cannot rule it out, run the backtracker") whenever the
/// linear program is not authoritative: not built, or the never-match
/// placeholder installed for a pattern the `regex` crate could not compile at
/// all. That is what makes the gate safe for the lookaround shapes, which have
/// no linear program to consult.
///
/// This does NOT bound the worst case; it removes the reachable one. A real
/// step budget has to be counted by the backtracker — `regress` has none
/// today, `fancy-regex` ships `backtrack_limit: 1_000_000` — and is pending
/// upstream.
#[cfg(feature = "regex-engine")]
fn linear_rules_out_match(re: *const RegExpHeader, subject: &str, start: usize) -> bool {
    unsafe {
        let program = (*re).regex_ptr;
        if program.is_null() {
            return false;
        }
        let program: &Regex = &*program;
        if program.as_str() == NEVER_MATCH_PATTERN {
            // The `regex` crate refused this pattern (lookaround /
            // backreference); it has no opinion about the subject.
            return false;
        }
        start <= subject.len() && !program.is_match_at(subject, start)
    }
}

/// [`lookup_repeat_matcher`] with the linear pre-check applied: `None` also
/// when the linear program proves no match at or after `start`, so the
/// backtracker is never entered on a subject that cannot match. Every
/// `&str`-subject call site uses this; the WTF-8/UTF-16 replace path, which has
/// no `&str` to hand, uses the bare lookup.
#[cfg(feature = "regex-engine")]
fn lookup_repeat_matcher_for(
    re: *const RegExpHeader,
    subject: &str,
    start: usize,
) -> Option<Arc<repeat_matcher::RepeatMatcherRegex>> {
    let matcher = lookup_repeat_matcher(re)?;
    if linear_rules_out_match(re, subject, start) {
        return None;
    }
    Some(matcher)
}

/// Look up the ECMAScript-native matcher used when quantified capture groups
/// make `RepeatMatcher`'s capture-reset semantics observable.
#[cfg(feature = "regex-engine")]
fn lookup_repeat_matcher(
    re: *const RegExpHeader,
) -> Option<Arc<repeat_matcher::RepeatMatcherRegex>> {
    // Same first-use build as `lookup_fancy_regex`: a null
    // `repeat_matcher_ptr` means "not built yet" before it can mean "this
    // pattern needs no backtracking matcher".
    lazy::ensure_regex_compiled(re);
    unsafe {
        if regex_header_has_magic(re) {
            if (*re).repeat_matcher_ptr.is_null() {
                // Same reasoning as `lookup_fancy_regex`: a built header with
                // a null pointer has no backtracking matcher.
                return None;
            }
            let raw = (*re).repeat_matcher_ptr as *const repeat_matcher::RepeatMatcherRegex;
            let arc = Arc::from_raw(raw);
            let cloned = arc.clone();
            std::mem::forget(arc);
            return Some(cloned);
        }
        let pat = string_as_str((*re).pattern_ptr);
        let flags_str = string_as_str((*re).flags_ptr);
        REPEAT_MATCHER_CACHE.with(|cache| {
            cache
                .borrow()
                .get(&(Arc::from(pat), Arc::from(flags_str)))
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
    let out = format!("/{}/{}", string_as_str(src), string_as_str(flg));
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
#[cfg(all(test, feature = "regex-engine"))]
mod tests_part2;
