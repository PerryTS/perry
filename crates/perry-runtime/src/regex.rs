//! RegExp runtime support for Perry
//!
//! JavaScript-compatible regular expressions compiled and executed by Perex.
//! RegExp objects are heap-allocated and store the compiled pattern and flags.

use std::cell::RefCell;
use std::ptr;

#[cfg(feature = "regex-engine")]
use crate::array::ArrayHeader;
use crate::string::StringHeader;

use crate::object::ObjectHeader;

#[cfg(feature = "regex-engine")]
mod compile;
mod escape;
#[cfg(feature = "regex-engine")]
mod flags;
#[cfg(feature = "regex-engine")]
mod perex_split_compat;
#[cfg(feature = "regex-engine")]
pub use perex_split_compat::{js_string_split_n, js_string_split_regex, js_string_split_regex_n};
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_split;
#[cfg(feature = "regex-engine")]
pub use perex_split::js_string_split_js;
#[cfg(feature = "regex-engine")]
pub(crate) mod match_all;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_api;
#[cfg(feature = "regex-engine")]
pub(crate) mod site_test;
#[cfg(feature = "regex-engine")]
mod perex_construct;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_dispatch;
#[cfg(feature = "regex-engine")]
mod perex_display;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_glob;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_literal_bytes;
#[cfg(feature = "regex-engine")]
mod perex_literal_search;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_match_search;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_memory;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_owner;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_replace;
#[cfg(feature = "regex-engine")]
mod perex_replace_storage;
#[cfg(feature = "regex-engine")]
mod perex_substitution;
#[cfg(feature = "regex-engine")]
pub use perex_replace::{js_string_replace_all_js, js_string_replace_js};
#[cfg(feature = "regex-engine")]
mod perex_replace_compat;
#[cfg(feature = "regex-engine")]
mod perex_results;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_runtime;
#[cfg(feature = "regex-engine")]
pub(crate) mod perex_strings;
#[cfg(not(feature = "regex-engine"))]
mod replace_fn;
mod utf16;
#[cfg(feature = "regex-engine")]
pub use compile::js_regexp_compile_value;
#[cfg(not(feature = "regex-engine"))]
use escape::escape_regexp_source;
pub use escape::js_regexp_escape;
#[cfg(feature = "regex-engine")]
pub(crate) use flags::validate_and_canonicalize_flags;
#[cfg(feature = "regex-engine")]
pub(crate) use match_all::dispatch_regexp_string_iterator_method_builtin;
#[cfg(all(test, feature = "regex-engine"))]
pub use match_all::js_string_match_all;
#[cfg(feature = "regex-engine")]
pub use match_all::{
    dispatch_regexp_string_iterator_method, js_string_match_all_js, js_string_match_all_value,
};
#[cfg(all(test, feature = "regex-engine"))]
use utf16::{byte_index_to_utf16_index, utf16_index_to_byte};

/// Class id for `RegExp String Iterator` exotic objects. Referenced by the
/// always-linked iterator-prototype dispatch, so it stays ungated even when
/// the regex engine (which produces these iterators) is compiled out.
pub const REGEXP_STRING_ITERATOR_CLASS_ID: u32 = 0xFFFF_000A;
#[cfg(feature = "regex-engine")]
pub use perex_replace_compat::*;
#[cfg(not(feature = "regex-engine"))]
pub use replace_fn::{
    js_string_replace_all_js, js_string_replace_all_string, js_string_replace_all_string_fn,
    js_string_replace_js, js_string_replace_string, js_string_replace_string_fn,
};
#[cfg(feature = "regex-engine")]
mod exec;
#[cfg(feature = "regex-engine")]
mod match_string;
#[cfg(feature = "regex-engine")]
pub use exec::js_regexp_exec;
#[cfg(all(test, feature = "regex-engine"))]
pub use match_string::{js_string_match, js_string_search_regex};
#[cfg(feature = "regex-engine")]
pub use match_string::{
    js_string_match_js, js_string_match_value, js_string_search_js, js_string_search_value,
};

/// Local owner registration. Source and flags live only in the header's
/// traced string edges; metadata never keeps native copies of either string.
struct RegexMetadata {
    registered_owner: bool,
}

crate::perry_thread_local! {
    #[cfg(feature = "regex-engine")]
    static LAST_EXEC_INDEX: RefCell<f64> = const { RefCell::new(0.0) };

    static LAST_EXEC_GROUPS: RefCell<*mut ObjectHeader> = const { RefCell::new(ptr::null_mut()) };

    /// Headers constructed in this runtime participate in collector owner
    /// walks. The historical table name remains while legacy callers migrate.
    static REGEX_SOURCE_TABLE: RefCell<crate::fast_hash::PtrHashMap<usize, RegexMetadata>> = RefCell::new(crate::fast_hash::new_ptr_hash_map());
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
/// The three owner-registration probes all reach the thread-local table only
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
    REGEX_SOURCE_TABLE.with(|table| {
        table
            .borrow()
            .get(&addr)
            .is_some_and(|entry| entry.registered_owner)
    })
}

/// Rekey every address-owned RegExp table after payload evacuation. Header
/// child slots are rewritten separately by the RegExp GC descriptor; this
/// hook handles the owner keys that a slot visitor cannot see.
pub(crate) fn regex_header_moved_for_gc(old_addr: usize, new_addr: usize) {
    if old_addr == new_addr {
        return;
    }
    REGEX_SOURCE_TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if let Some(mut metadata) = table.remove(&old_addr) {
            match table.entry(new_addr) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    // The former set retained destination registration too;
                    // the source metadata still comes from the moved owner.
                    metadata.registered_owner |= entry.get().registered_owner;
                    entry.insert(metadata);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(metadata);
                }
            }
        }
    });
    crate::object::exotic_expando::exotic_expando_owner_moved(old_addr, new_addr);
}

/// Remove address-owned RegExp metadata when the cell is proven dead.
pub(crate) fn regex_header_clear_dead_for_gc(addr: usize) {
    REGEX_SOURCE_TABLE.with(|table| {
        table.borrow_mut().remove(&addr);
    });
    crate::object::exotic_expando::exotic_expando_owner_clear_dead(addr);
}

/// Remove a dead header's address-owned metadata. Its program and strings are
/// ordinary traced GC children and are reclaimed by the collector.
pub(crate) unsafe fn regex_header_finalize_for_gc(re: *mut RegExpHeader) {
    if !re.is_null() {
        regex_header_clear_dead_for_gc(re as usize);
    }
}

/// Finalize the RegExp headers that died in from-space during a copied minor.
///
/// The copying minor's from-space flip runs no per-object finalize hooks, so
/// a nursery header that was neither evacuated nor pinned would otherwise keep
/// its source/registration metadata and expando
/// entries forever. Same shape as `map::finalize_dead_copied_minor_from_space_maps`:
/// walk the registry after the flip, collect the provably-dead addresses, then
/// finalize each (the finalizer removes its own registry entries, which is why
/// the walk and the removal are two passes).
///
/// Cost: O(registry) = O(live headers + headers allocated since the last
/// minor) — the same order as the malloc sweep this replaces, and
/// proportional to allocation, not to program history.
pub(crate) fn finalize_dead_copied_minor_from_space_regexps() -> usize {
    let dead: Vec<usize> = REGEX_SOURCE_TABLE.with(|table| {
        let table = table.borrow();
        let owners = table
            .iter()
            .filter_map(|(&addr, entry)| entry.registered_owner.then_some(addr));
        crate::gc::prefetch::prefetch_gc_owner_headers(owners)
            .filter(|&addr| {
                crate::gc::owner_is_dead_copied_minor_from_space_of_type(
                    addr,
                    crate::gc::GC_TYPE_REGEXP,
                )
            })
            .collect()
    });
    let count = dead.len();
    for addr in crate::gc::prefetch::prefetch_gc_owner_headers(dead.iter().copied()) {
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
    REGEX_SOURCE_TABLE.with(|table| {
        table
            .borrow()
            .iter()
            .filter_map(|(&addr, entry)| entry.registered_owner.then_some(addr))
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
    let re = scope.root_raw_mut_ptr(re);
    let subject = scope.root_string_ptr(js_string_from_str("abc"));
    subject.with_const_ptr::<StringHeader, _>(|s| {
        let _ = js_regexp_test(re.get_raw_const_ptr::<RegExpHeader>(), s);
    });
    re.get_raw_mut_ptr::<RegExpHeader>()
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_original_strings_and_program(
    re: *const RegExpHeader,
) -> (*const StringHeader, *const StringHeader, bool) {
    unsafe {
        let program = crate::value::addr_class::try_read_gc_header((*re).perex_program as usize);
        (
            (*re).pattern_ptr,
            (*re).flags_ptr,
            program.is_some_and(|gc| gc.obj_type == crate::gc::GC_TYPE_REGEX_PROGRAM),
        )
    }
}

#[cfg(all(test, feature = "regex-engine"))]
pub(crate) fn test_regexp_program_address(re: *const RegExpHeader) -> usize {
    unsafe { (*re).perex_program as usize }
}

#[cfg(test)]
pub(crate) fn test_regex_pointer_entry_exists(addr: usize) -> bool {
    REGEX_SOURCE_TABLE.with(|table| {
        table
            .borrow()
            .get(&addr)
            .is_some_and(|entry| entry.registered_owner)
    })
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let source = scope.root_string_ptr(js_string_from_str(source));
    let flags_root = scope.root_string_ptr(js_string_from_str(flags));
    unsafe {
        let ptr = crate::arena::arena_alloc_gc(
            std::mem::size_of::<RegExpHeader>(),
            std::mem::align_of::<RegExpHeader>(),
            crate::gc::GC_TYPE_REGEXP,
        ) as *mut RegExpHeader;
        // Neither `gc_malloc` nor the arena zeroes reused memory, so this
        // must be set explicitly or the GC follows a garbage pointer.
        (*ptr).meta = std::ptr::null_mut();
        (*ptr).pattern_ptr = source.get_raw_const_ptr::<StringHeader>();
        (*ptr).flags_ptr = flags_root.get_raw_const_ptr::<StringHeader>();
        (*ptr).perex_program = std::ptr::null();
        (*ptr).case_insensitive = flags.contains('i');
        (*ptr).global = flags.contains('g');
        (*ptr).multiline = flags.contains('m');
        (*ptr).sticky = flags.contains('y');
        (*ptr).dot_all = flags.contains('s');
        (*ptr).unicode = flags.contains('u') || flags.contains('v');
        (*ptr).has_indices = flags.contains('d');
        (*ptr).last_index = crate::value::JSValue::number(0.0).bits();
        (*ptr).magic = REGEXP_MAGIC;

        REGEX_EVER_REGISTERED.arm();
        REGEX_SOURCE_TABLE.with(|table| {
            table.borrow_mut().insert(
                ptr as usize,
                RegexMetadata {
                    registered_owner: true,
                },
            );
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

/// The source/flags range and lastIndex slot of a `RegExpHeader`:
///   * `pattern_ptr` — the original-source `StringHeader`,
///   * `flags_ptr`   — the flags `StringHeader`,
///   * `last_index`  — a writable JSValue (`re.lastIndex = …`) that may be a
///     NaN-boxed heap pointer.
/// The layout visitor separately enumerates `meta` and the GC-managed
/// `perex_program` edge. Boolean flags and `magic` are not GC edges.
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

/// Header for heap-allocated RegExp objects
#[repr(C)]
pub struct RegExpHeader {
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
    /// rely SOLELY on thread-local owner registration. That breaks when a
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
    /// Storing the marker and traced program on the heap header keeps identity
    /// and execution independent of the runtime copy performing dispatch.
    pub magic: u64,
    /// #6759 phase 1 (header unification): per-object metadata record, or
    /// null. Its edge is found through the actual Rust struct layout.
    ///
    /// RegExp's rewrite descriptor DELEGATES to the layout visitor, so unlike
    /// Error/Map/Set the edge belongs in `gc_child_slots`
    /// (`GcLayoutSlotKind::RegExpFields`) — that is the marking path here.
    /// #6812 is precisely the bug of putting it in the wrong one.
    pub meta: *mut crate::object::ObjectMeta,
    /// The single immutable GC-managed program, traced and rewritten by the
    /// RegExp layout visitor.
    pub(crate) perex_program: *const u8,
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let stored = f64::from_bits(unsafe { (*re).last_index });
    let stored = scope.root_nanbox_f64(stored);
    perex_api::finish(perex_dispatch::to_length(&stored)) as usize
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
/// header we allocated via `js_regexp_new` (recorded as a registered owner).
/// The LLVM backend's `new RegExp(pat, flags)` currently falls through
/// to the generic `lower_new` path which allocates an empty object and
/// NaN-boxes it as a regex; subsequent `.exec()` / `.test()` calls would
/// read garbage from that object if we didn't gate them on this check.
#[inline]
pub(crate) fn is_valid_regex_ptr(p: *const RegExpHeader) -> bool {
    #[cfg(test)]
    REGEX_PTR_VALIDATION_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    if !is_valid_ptr(p) {
        return false;
    }
    // Wall 18: header magic first (duplicate-runtime thread-local resilient).
    if regex_header_has_magic(p) {
        return true;
    }
    regex_pointers_contains(p as usize)
}

#[cfg(test)]
static REGEX_PTR_VALIDATION_CALLS: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

/// How many times a pointer has been validated. A bounded view call must
/// validate its regex exactly once; counting is how that stays true.
#[cfg(test)]
pub(crate) fn test_regex_ptr_validation_calls() -> u64 {
    REGEX_PTR_VALIDATION_CALLS.load(std::sync::atomic::Ordering::Relaxed)
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

/// Create a new RegExp from pattern and flags strings
/// Returns a pointer to RegExpHeader
///
/// Compile the original pattern with Perex before publishing a fresh object.
/// Source and flags remain traced string edges; the compiled program is a GC
/// leaf owned through the header. Every evaluation creates a distinct header.
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_new(
    pattern: *const StringHeader,
    flags: *const StringHeader,
) -> *mut RegExpHeader {
    js_regexp_new_impl(pattern, flags, 0)
}

/// Literal construction entry point retained for the current generated ABI.
/// Until AOT program emission is connected, this uses the same Perex compiler
/// as dynamic construction. The old site cache is no longer a constructor path.
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
    _site_key: usize,
) -> *mut RegExpHeader {
    perex_api::finish(perex_construct::new(pattern, flags))
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
    perex_construct::construct(pattern, flags, false)
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
    perex_construct::construct(pattern, flags, true)
}

/// Test if a string matches the regex pattern
/// regex.test(string) -> boolean
#[cfg(feature = "regex-engine")]
#[no_mangle]
pub extern "C" fn js_regexp_test(re: *const RegExpHeader, s: *const StringHeader) -> i32 {
    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        return 0;
    }
    if crate::hot_diag::regex_on() {
        diag_note_op(re, crate::hot_diag::RegexOp::Test);
    }
    i32::from(perex_api::finish(perex_dispatch::test_string(
        crate::value::js_nanbox_pointer(re as i64),
        s,
    )))
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

/// Dispatch methods on a registered RegExp receiver.
#[cfg(feature = "regex-engine")]
pub(crate) fn dispatch_regex_receiver_method(
    ptr: *const u8,
    method: &str,
    arg0: f64,
) -> Option<f64> {
    if !is_regex_pointer(ptr) {
        return None;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let re = scope.root_raw_const_ptr(ptr as *const RegExpHeader);
    match method {
        "test" => {
            let s_ptr = crate::value::js_jsvalue_to_string_coerce(arg0);
            let matched = js_regexp_test(re.get_raw_const_ptr(), s_ptr) != 0;
            Some(f64::from_bits(crate::value::JSValue::bool(matched).bits()))
        }
        // exec: the match array, or `null` on no match (spec-correct).
        "exec" => {
            let s_ptr = crate::value::js_jsvalue_to_string_coerce(arg0);
            let arr = js_regexp_exec(re.get_raw_mut_ptr(), s_ptr);
            Some(if arr.is_null() {
                f64::from_bits(crate::value::TAG_NULL)
            } else {
                f64::from_bits(crate::value::JSValue::pointer(arr as *const u8).bits())
            })
        }
        // `regex.toString()` → `/source/flags` (RegExp.prototype.toString).
        "toString" => {
            let s = js_regexp_to_string(re.get_raw_const_ptr());
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
    #[cfg(feature = "regex-engine")]
    {
        return perex_api::finish(perex_display::source(re));
    }
    #[cfg(not(feature = "regex-engine"))]
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
    unsafe {
        let flags = (*re).flags_ptr;
        if !is_valid_ptr(flags) {
            return js_string_from_str("");
        }
        crate::string::js_string_addref(flags as *mut StringHeader);
        flags as *mut StringHeader
    }
}

/// `RegExp.prototype.toString()` — `/source/flags`. Used by both the
/// `regex.toString()` method dispatch and ToString coercion (`String(re)`,
/// template literals). Node never produces `"[object Object]"` for a RegExp.
#[no_mangle]
pub extern "C" fn js_regexp_to_string(re: *const RegExpHeader) -> *mut StringHeader {
    #[cfg(feature = "regex-engine")]
    {
        return perex_api::finish(perex_display::to_string(re));
    }
    #[cfg(not(feature = "regex-engine"))]
    {
        let scope = crate::gc::RuntimeHandleScope::new();
        let re = scope.root_raw_const_ptr(re);
        let src = scope.root_string_ptr(js_regexp_get_source(re.get_raw_const_ptr()));
        let flg = scope.root_string_ptr(js_regexp_get_flags(re.get_raw_const_ptr()));
        let out = format!(
            "/{}/{}",
            string_as_str(src.get_raw_const_ptr()),
            string_as_str(flg.get_raw_const_ptr())
        );
        js_string_from_str(&out)
    }
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
        crate::gc::runtime_write_barrier_gc_slot(
            re as usize,
            std::ptr::addr_of!((*re).last_index) as usize,
            value.to_bits(),
        );
    }
}

#[cfg(all(test, feature = "regex-engine"))]
mod tests;
#[cfg(all(test, feature = "regex-engine"))]
mod tests_part2;
