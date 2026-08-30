//! Symbol runtime support for Perry
//!
//! Minimal Symbol implementation providing:
//! - `Symbol()` / `Symbol(description)` — unique symbol creation
//! - `Symbol.for(key)` — global registry (interned symbols)
//! - `Symbol.keyFor(sym)` — reverse lookup (returns undefined for non-registered)
//! - `sym.description` — original description string
//! - `sym.toString()` — "Symbol(description)"
//! - `Object.getOwnPropertySymbols(obj)` — always returns an empty array (real
//!   symbol-keyed properties are not yet wired into the object shape system)
//!
//! Symbols are opaque heap objects allocated via `gc_malloc` with
//! `GC_TYPE_STRING` (treated as leaf objects by the GC — no internal
//! references). They are NaN-boxed with `POINTER_TAG`, which means they
//! round-trip through the runtime as regular pointer JSValues.
//!
//! Dedicated Symbol support requires a small codegen hook (see report):
//! intercepting `Symbol(desc)` / `Symbol.for(key)` / `Symbol.keyFor(sym)` /
//! `Object.getOwnPropertySymbols(obj)` calls and routing them to the
//! functions in this module.

mod accessors;
#[cfg(test)]
pub(crate) use accessors::{
    test_seed_symbol_accessor_property, test_symbol_accessor_property_count,
};
mod constructors;
mod gc_roots;
mod get;
mod iterator;
mod properties;

pub(crate) use accessors::set_symbol_accessor_property;

// Symbol constructor + value FFI (no_mangle entry points re-exported so existing
// `crate::symbol::js_symbol_*` call paths keep resolving).
pub use constructors::{
    js_symbol_description, js_symbol_equals, js_symbol_for, js_symbol_key_for, js_symbol_new,
    js_symbol_new_empty, js_symbol_to_string, js_symbol_typeof,
};

// Symbol-keyed property side-table operations.
pub(crate) use properties::{
    class_static_symbol_keys_for_class, clone_symbol_entries_for_obj_ptr,
    define_symbol_data_property, get_symbol_property_attrs, inspect_custom_symbol_ptr,
    js_object_define_symbol_accessor, js_object_delete_symbol_property,
    js_object_has_own_symbol_property, reflect_symbol_getter_closure_bits,
    set_symbol_property_attrs, symbol_accessor_descriptor_bits, symbol_property_is_enumerable,
    symbol_property_is_non_writable, symbol_property_root_bits,
};
pub use properties::{
    class_static_symbol_lookup, js_class_register_static_symbol, js_object_has_own_symbol,
    js_object_literal_infer_computed_function_name, js_object_set_method_by_name,
    js_object_set_symbol_method, js_object_set_symbol_property,
};

// Symbol-keyed property reads.
pub(crate) use get::{has_own_symbol_property, inherited_symbol_property, own_symbol_property};
pub use get::{
    js_object_get_symbol_property, js_object_get_symbol_property_ic_miss,
    js_object_get_symbol_then_field_ic_miss,
};

// Iterator protocol, getOwnPropertySymbols, ToPrimitive.
pub(crate) use iterator::class_ref_resolves_iterator;
pub use iterator::{
    js_get_iterator, js_iterator_result_validate, js_object_get_own_property_symbols,
    js_to_primitive,
};

// GC root scanning + incremental snapshot driver.
pub(crate) use gc_roots::{
    new_symbol_side_table_root_scan_state, scan_symbol_side_table_roots_mut_step,
};
pub use gc_roots::{scan_symbol_side_table_roots, scan_symbol_side_table_roots_mut};

#[cfg(test)]
pub(crate) use gc_roots::{
    test_class_static_symbol_root_bits, test_class_static_symbol_roots_for_class,
    test_clear_symbol_side_table_roots, test_seed_class_static_symbol_root,
    test_seed_symbol_pointer_root, test_seed_symbol_property_root,
    test_symbol_pointer_root_contains, test_symbol_property_owner_exists,
    test_symbol_property_root_bits, test_symbol_property_roots,
};

use crate::fast_hash::{
    new_ptr_hash_map, new_ptr_hash_set, FastKeyHashMap, PtrHashMap, PtrHashSet,
};
use crate::string::StringHeader;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;

// NaN-boxing tags (must match value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Invalidates generated weak Symbol-property inline caches.
///
/// The caches deliberately hold raw NaN-boxed bits without registering them as
/// roots: making a receiver/value reachable only through an optimization cache
/// survive collection would change WeakRef/finalization behavior. Every
/// symbol-data mutation and every completed collection advances this epoch, so
/// cached raw addresses are observed only while the heap and the corresponding
/// own data property are unchanged.
#[no_mangle]
pub static PERRY_SYMBOL_PROPERTY_IC_EPOCH: AtomicU64 = AtomicU64::new(1);

#[inline]
pub(crate) fn symbol_property_ic_epoch_bump() {
    PERRY_SYMBOL_PROPERTY_IC_EPOCH.fetch_add(1, Ordering::Release);
}

/// Magic number distinguishing SymbolHeader from other GC_TYPE_STRING objects.
/// Placed at offset 0 so `js_is_symbol` can cheaply detect symbols.
pub const SYMBOL_MAGIC: u32 = 0x5359_4D42; // "SYMB"

/// Symbol object header. Allocated via `gc_malloc` (or malloc for registered
/// symbols that need to outlive GC cycles).
#[repr(C)]
pub struct SymbolHeader {
    /// Magic number for type discrimination. Always SYMBOL_MAGIC.
    pub magic: u32,
    /// Whether this symbol is in the global registry (Symbol.for). Registered
    /// symbols have their description used as the registry key.
    pub registered: u32,
    /// Description string pointer, or null for `Symbol()` with no argument.
    pub description: *mut StringHeader,
    /// Unique id (monotonic counter). Two symbols with the same description
    /// still compare as different unless created via Symbol.for.
    pub id: u64,
}

// Global registry for Symbol.for(key) — maps key → symbol pointer (as usize).
// The symbol pointers stored here are leaked (never freed) so that
// `Symbol.for("x") === Symbol.for("x")` always returns the same pointer.
static SYMBOL_REGISTRY: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

// Side-table tracking ALL allocated symbol pointers (both gc_malloc'd from
// `Symbol(desc)` and Box::leak'd from `Symbol.for(key)`). Used by
// `is_registered_symbol` so the runtime's property/method dispatch can
// detect symbol pointers safely without reading the (possibly nonexistent)
// GcHeader byte.
per_test_global! {
    static SYMBOL_POINTERS: Mutex<Option<PtrHashSet<usize>>> = Mutex::new(None);
}

/// Process-lifetime descriptions for registered (`Symbol.for`) and well-known
/// symbols. These symbols are Box-leaked so they outlive every GC cycle, but
/// the description StringHeader they used to point at was allocated in the
/// calling thread's arena — which gets freed when a `perry/thread` worker
/// exits, leaving the symbol with a dangling description pointer. Storing
/// the description text here (Rust-owned, process-lifetime) lets readers
/// materialize a fresh StringHeader in the *caller's* arena on demand, which
/// is the only thread-safe contract: the symbol identity is global, but
/// every StringHeader belongs to exactly one thread's arena.
static REGISTERED_SYMBOL_DESCRIPTIONS: Mutex<Option<PtrHashMap<usize, std::sync::Arc<str>>>> =
    Mutex::new(None);

pub(crate) fn registered_symbol_description(sym_ptr: usize) -> Option<std::sync::Arc<str>> {
    let guard = REGISTERED_SYMBOL_DESCRIPTIONS.lock().unwrap();
    guard.as_ref().and_then(|m| m.get(&sym_ptr).cloned())
}

crate::perry_thread_local! {
    /// ★ #7246: descriptions of FRESH (`Symbol("x")`) symbols, off the GC heap.
    ///
    /// A `SymbolHeader` used to store its description as a `*mut StringHeader`
    /// in the payload — and **the collector never traced or rewrote it**.
    /// `alloc_symbol` gc_malloc's the header as `GC_TYPE_STRING`, whose type
    /// info is `pointer_free: true` / `GcRewriteDescriptorKind::Leaf` /
    /// `GcLayoutSlotKind::None`. That is correct for a *string*, whose payload
    /// is bytes; it is wrong for a *symbol*, whose payload's third word is a
    /// heap pointer. Symbols and strings share one GC type, so no descriptor
    /// could tell them apart. A perfectly rooted symbol could therefore have
    /// its description reaped or relocated out from under it, and
    /// `String(sym)` / `sym.description` then read recycled memory.
    ///
    /// The pointer is gone rather than traced. Three fixes were on the table
    /// (#7246): a `GC_TYPE_SYMBOL` with a real descriptor, tracing the
    /// description from the symbol side table, or interning it off-heap. This
    /// is the third, and the reason it is cheap is the KEY:
    ///
    ///   * keyed on `SymbolHeader::id` — a monotonic `u64` that an evacuation
    ///     copies verbatim — **not** on the symbol's address. So this table
    ///     needs no rekey pass, no root scanner, and no budgeted step twin. It
    ///     holds no GC pointer at all, which is why
    ///     `scripts/gc_runtime_root_holders.py` will not ask it for a verdict;
    ///   * `alloc_symbol` copies the text BEFORE it allocates, so there is no
    ///     window in which a description pointer is live-but-untraced;
    ///   * it is pruned alongside `SYMBOL_POINTERS` in
    ///     `prune_dead_symbol_pointers`, so a symbol-churn loop does not retain
    ///     one `Arc<str>` per symbol forever.
    ///
    /// The process-global `REGISTERED_SYMBOL_DESCRIPTIONS` above stays as it
    /// is: registered and well-known symbols are `Box::leak`'d and shared
    /// across `perry/thread` agents, so their descriptions must be
    /// process-global. Fresh symbols are per-thread GC objects, so theirs are
    /// thread-local. Ids are globally monotonic, so the two never collide.
    /// Stored as raw BYTES, not `str`. `str_from_header` UTF-8-validates and
    /// returns `None` on failure, and a description built from a JS string with
    /// a lone surrogate is WTF-8, not UTF-8. Interning through `String` would
    /// therefore have turned a lone-surrogate `sym.description` into
    /// `undefined` — a behaviour change smuggled in on a GC fix. Raw bytes
    /// round-trip through `js_string_from_bytes` unchanged.
    ///
    /// Residual, stated rather than hidden: the rebuilt `StringHeader` does not
    /// carry `STRING_FLAG_HAS_LONE_SURROGATES`, because the original flag is
    /// not recoverable from the payload. That is the pre-existing WTF-8 gap
    /// CLAUDE.md already lists, not a new one, and it is strictly better than
    /// dropping the description.
    static FRESH_SYMBOL_DESCRIPTIONS: RefCell<PtrHashMap<u64, std::sync::Arc<[u8]>>> =
        RefCell::new(new_ptr_hash_map());
}

#[cfg(test)]
pub(crate) fn test_clear_fresh_symbol_descriptions() {
    FRESH_SYMBOL_DESCRIPTIONS.with(|m| m.borrow_mut().clear());
}

/// The description text of `sym_ptr`, wherever it is kept.
///
/// One helper rather than the `registered_symbol_description(..).or_else(..)`
/// chain each reader used to spell out: there are four readers, and a fifth
/// that forgot the fallback is exactly how a description goes silently missing.
pub(crate) unsafe fn symbol_description_text(
    sym_ptr: *const SymbolHeader,
) -> Option<std::sync::Arc<[u8]>> {
    // #1843/#6271: a bare `< 0x1000` floor does not reject the fetch/zlib/proxy
    // handle bands, and dereferencing one segfaults on Linux while macOS hides
    // it. `is_above_handle_band` is the predicate that does.
    if sym_ptr.is_null() || !crate::value::addr_class::is_above_handle_band(sym_ptr as usize) {
        return None;
    }
    if let Some(text) = registered_symbol_description(sym_ptr as usize) {
        return Some(std::sync::Arc::from(text.as_bytes()));
    }
    let id = (*sym_ptr).id;
    if let Some(text) = FRESH_SYMBOL_DESCRIPTIONS.with(|m| m.borrow().get(&id).cloned()) {
        return Some(text);
    }
    // Legacy fallback: any symbol whose description still lives in the payload
    // (nothing populates this today — `alloc_symbol` nulls it — but the field
    // is still readable and a stale reader would otherwise silently return
    // `None` instead of a description).
    description_bytes_from_header((*sym_ptr).description).map(std::sync::Arc::from)
}

/// SetFunctionName spelling for a Symbol property key: an undefined
/// description produces the empty string, otherwise `[description]`.
pub(crate) unsafe fn symbol_function_name(sym_key: usize) -> String {
    let sym_ptr = sym_key as *const SymbolHeader;
    match symbol_description_text(sym_ptr) {
        Some(desc) => format!("[{}]", String::from_utf8_lossy(desc.as_ref())),
        None => String::new(),
    }
}

/// The raw payload bytes of a description `StringHeader`, WITHOUT UTF-8
/// validation. `str_from_header` validates and would drop a WTF-8 description
/// on the floor (#7246).
unsafe fn description_bytes_from_header(ptr: *const StringHeader) -> Option<Vec<u8>> {
    // As above — this dereferences `ptr`, so the handle bands must be excluded.
    if ptr.is_null() || !crate::value::addr_class::is_above_handle_band(ptr as usize) {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    Some(std::slice::from_raw_parts(data, len).to_vec())
}

fn record_fresh_symbol_description(id: u64, description: &[u8]) {
    FRESH_SYMBOL_DESCRIPTIONS
        .with(|m| m.borrow_mut().insert(id, std::sync::Arc::from(description)));
}

#[cfg(test)]
pub(crate) fn test_fresh_symbol_description_count() -> usize {
    FRESH_SYMBOL_DESCRIPTIONS.with(|m| m.borrow().len())
}

pub(crate) fn record_registered_symbol_description(sym_ptr: usize, description: &str) {
    let mut guard = REGISTERED_SYMBOL_DESCRIPTIONS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(new_ptr_hash_map());
    }
    guard
        .as_mut()
        .unwrap()
        .insert(sym_ptr, std::sync::Arc::from(description));
}

// Pre-allocated well-known symbols (Symbol.toPrimitive, Symbol.hasInstance,
// Symbol.match, Symbol.toStringTag, Symbol.iterator, Symbol.asyncIterator,
// Symbol.species, and the string/regexp protocol symbols). Allocated once
// on first access and cached forever. These are distinct from the
// `Symbol.for(key)` registry — `Symbol.keyFor(wk)` must return undefined
// for spec compliance, so they live in their own map keyed by the
// well-known name ("toPrimitive" etc.).
//
// HIR lowers `Symbol.toPrimitive` to `Expr::SymbolFor(Expr::String("@@__perry_wk_toPrimitive"))`
// and the runtime's `js_symbol_for` sniffs the `@@__perry_wk_` prefix and
// returns the cached pointer.
pub(crate) const WK_PREFIX: &str = "@@__perry_wk_";
static WELL_KNOWN_SYMBOLS: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// Lazily allocate & cache a well-known symbol by its short name ("toPrimitive").
/// Returns the pointer to the cached `SymbolHeader`. Registered in
/// `SYMBOL_POINTERS` so `js_is_symbol` / `is_registered_symbol` recognize it.
pub fn well_known_symbol(short_name: &str) -> *mut SymbolHeader {
    let mut guard = WELL_KNOWN_SYMBOLS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let cache = guard.as_mut().unwrap();
    if let Some(&ptr_usize) = cache.get(short_name) {
        return ptr_usize as *mut SymbolHeader;
    }
    // First use: allocate a persistent (leaked) SymbolHeader. Description is
    // null-on-the-header — the actual text lives in REGISTERED_SYMBOL_DESCRIPTIONS,
    // and readers materialize a StringHeader in their own arena on demand. We
    // can't store a real StringHeader pointer here because this allocation may
    // be made on a worker thread whose arena will later be torn down, while
    // the SymbolHeader itself is Box-leaked and outlives that arena.
    let boxed = Box::new(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: 0,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    let sym_ptr = Box::into_raw(boxed);
    // Fully initialize the symbol's side tables BEFORE publishing it in
    // the cache. A concurrent reader that observes the pointer via the
    // cache must already see a complete view (description present,
    // is_registered_symbol true) — otherwise `Symbol.description` /
    // `Symbol.toString()` / `is_symbol` can transiently return wrong
    // results. Lock order matches `js_symbol_for` below: cache → side
    // tables, never the reverse.
    // Spec: a well-known symbol's `[[Description]]` is the qualified name
    // `"Symbol.iterator"`, not the bare `"iterator"`. This is what
    // `Symbol.iterator.description`, `.toString()`, `String(sym)`, and
    // `console.log` all report. The cache key stays the short name so callers
    // (`well_known_symbol("iterator")`) and pointer-identity property lookups
    // are unaffected.
    record_registered_symbol_description(sym_ptr as usize, &format!("Symbol.{short_name}"));
    register_symbol_pointer(sym_ptr as usize);
    cache.insert(short_name.to_string(), sym_ptr as usize);
    drop(guard);
    sym_ptr
}

/// Provider-safe C ABI for the Headers iterable probe. Separately packaged
/// stdlib images must not call the Rust-mangled `well_known_symbol` directly,
/// because their fallback runtime glue owns a different symbol cache.
#[no_mangle]
pub extern "C" fn js_symbol_well_known_iterator() -> f64 {
    let symbol = well_known_symbol("iterator");
    f64::from_bits(POINTER_TAG | (symbol as u64 & POINTER_MASK))
}

/// Provider-safe C ABI for separately linked native extensions that need to
/// expose a genuine async iterable.
#[no_mangle]
pub extern "C" fn js_symbol_well_known_async_iterator() -> f64 {
    let symbol = well_known_symbol("asyncIterator");
    f64::from_bits(POINTER_TAG | (symbol as u64 & POINTER_MASK))
}

/// O(1) check whether a raw pointer is a well-known symbol (Symbol.toPrimitive etc.).
/// Used by `js_symbol_key_for` so the spec-mandated `undefined` return for
/// well-known symbols is preserved.
pub fn is_well_known_symbol(ptr: usize) -> bool {
    let guard = WELL_KNOWN_SYMBOLS.lock().unwrap();
    if let Some(cache) = guard.as_ref() {
        for &p in cache.values() {
            if p == ptr {
                return true;
            }
        }
    }
    false
}

/// Monotone "this process has ever created a `Symbol`" latch.
///
/// `is_registered_symbol` is asked about ordinary pointer-shaped values on the
/// property/method dispatch paths, and until this latch existed it took a
/// *process-global* `Mutex` to answer — the most expensive miss in the type-probe
/// family, and 0.71% of an async-pipeline program that never mentions `Symbol`.
/// See `crate::registry_latch` for the ordering rule.
static SYMBOL_EVER_REGISTERED: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

/// True when the bytes at `ptr` *could* be a [`SymbolHeader`], i.e. when a
/// classifier that has already ruled a symbol out some other way must still ask
/// the authoritative [`is_registered_symbol`].
///
/// #7850. `gc_pointer_and_type_from_value` — on the path of every dynamic method
/// call — cannot use `GcHeader.obj_type` to rule a symbol out, because three of
/// the five registration sites (`well_known_symbol`,
/// `intl_legacy_constructed_symbol`, `js_symbol_for`) are `Box::into_raw`:
/// process-lifetime allocations with **no `GcHeader` at all**, so `ptr - 8` is
/// foreign allocator bytes that can coincidentally equal any `obj_type`. Trusting
/// the header for those is a silent wrong answer.
///
/// What every symbol DOES have, whatever its storage, is `SYMBOL_MAGIC` in its
/// own first four bytes — `alloc_symbol` and all three `Box` sites set it, and
/// the field is at offset 0 precisely so cheap discrimination is possible. So
/// one 4-byte load of the object the caller is already about to inspect answers
/// "definitely not a symbol" for everything else.
///
/// The direction of the guarantee is what makes it safe to use as a screen:
/// **`false` is exact** — no symbol reads `false` — while `true` is merely
/// "ask the registry". A non-symbol whose first word happens to equal
/// `SYMBOL_MAGIC` (a `StringHeader` would need `utf16_len == 0x5359_4D42`, i.e.
/// a 2.8 GB string; an `ObjectHeader`'s first word is `class_id`, and ids are
/// handed out from 1 — #8113 deleted the `object_type` tag that used to sit
/// there, which does not change this argument) simply pays the old probe and
/// gets the old, correct answer.
///
/// # Safety
/// `ptr` must be readable for 4 bytes. Every caller is one that already
/// dereferences the allocation (or its `GcHeader` at `ptr - 8`).
#[inline(always)]
pub(crate) unsafe fn may_be_symbol_header(ptr: *const u8) -> bool {
    #[cfg(test)]
    if TEST_DISABLE_SYMBOL_MAGIC_SCREEN.with(|c| c.get()) {
        return true;
    }
    std::ptr::read_unaligned(ptr as *const u32) == SYMBOL_MAGIC
}

#[cfg(test)]
thread_local! {
/// Test-only override that forces [`may_be_symbol_header`] to answer `true` —
/// i.e. removes the screen without deleting it, so a test can show the screen is
/// what makes the fast path fast rather than dead code in front of a probe that
/// would have answered anyway.
    static TEST_DISABLE_SYMBOL_MAGIC_SCREEN: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) fn test_disable_symbol_magic_screen(disabled: bool) -> bool {
    TEST_DISABLE_SYMBOL_MAGIC_SCREEN.with(|c| c.replace(disabled))
}

/// Smallest and largest pointer ever registered as a Symbol, as a conservative
/// filter in front of the process-global registry mutex.
///
/// `SYMBOL_EVER_REGISTERED` answers "has any symbol EVER been registered?",
/// which stops discriminating the moment a program creates its first symbol —
/// and every program that touches a well-known symbol creates one. After that
/// `is_registered_symbol_slow` took the global mutex on EVERY probe, including
/// the overwhelming majority asking about pointers that are not symbols at all.
///
/// The range only ever widens, and registration extends it before taking the
/// lock — the same ordering, and for the same reason, as the latch arm above
/// it: a pointer outside the range cannot be in the set, so rejecting is sound,
/// while accepting merely falls through to the lookup that was already there.
/// Death pruning removes entries without narrowing the range, which is
/// harmless: those pointers reach the lookup, which correctly says no.
static SYMBOL_ADDR_MIN: AtomicUsize = AtomicUsize::new(usize::MAX);
static SYMBOL_ADDR_MAX: AtomicUsize = AtomicUsize::new(0);

/// Widen the address range to admit `ptr`.
///
/// EVERY path that puts a pointer into `SYMBOL_POINTERS` must call this
/// first, not just the registration one. `is_registered_symbol_slow` rejects
/// an out-of-range pointer WITHOUT consulting the set, so a member outside
/// the range is a live symbol the probe reports as "not a symbol".
///
/// That is not a theoretical second inserter: `SYMBOL_POINTERS` is a GC root
/// registry, and `rewrite_symbol_pointer_metadata_if_forwarded` re-keys an
/// entry to the symbol's new address every time the collector moves it. A
/// symbol evacuated out of the range established by its own allocation would
/// otherwise stop answering to `typeof`, symbol-keyed property lookup and
/// `Symbol.iterator` dispatch — while still being perfectly alive.
pub(crate) fn widen_symbol_addr_range(ptr: usize) {
    SYMBOL_ADDR_MIN.fetch_min(ptr, Ordering::Release);
    SYMBOL_ADDR_MAX.fetch_max(ptr, Ordering::Release);
}

/// The ONLY way to put a pointer into `SYMBOL_POINTERS`. Widening and
/// inserting are one operation on purpose: there are three insert sites (the
/// registration, and two forwarding rewrites — a per-slot one and the bulk
/// one the copying minor actually drives), and a range filter is only sound
/// while every one of them widens.
pub(crate) fn insert_symbol_pointer_in_set(set: &mut PtrHashSet<usize>, ptr: usize) {
    widen_symbol_addr_range(ptr);
    set.insert(ptr);
}

/// Save/restore the range filter's bounds around a test that needs to observe
/// a NARROW range. The bounds are process-global and only ever widen in
/// production, so a test that resets them must put back at least what it
/// found — otherwise a later test in the same binary gets a range too narrow
/// for symbols registered before it ran, and fails for no reason of its own.
#[cfg(test)]
pub(crate) struct SymbolAddrRangeGuard(usize, usize);

#[cfg(test)]
impl SymbolAddrRangeGuard {
    /// Reset to the empty range, so only what the test registers is admitted.
    pub(crate) fn reset() -> Self {
        let g = SymbolAddrRangeGuard(
            SYMBOL_ADDR_MIN.load(Ordering::Acquire),
            SYMBOL_ADDR_MAX.load(Ordering::Acquire),
        );
        SYMBOL_ADDR_MIN.store(usize::MAX, Ordering::Release);
        SYMBOL_ADDR_MAX.store(0, Ordering::Release);
        g
    }
}

#[cfg(test)]
impl Drop for SymbolAddrRangeGuard {
    fn drop(&mut self) {
        // Union of what we saved and what the test widened to, so neither the
        // pre-existing members nor the test's own survive outside the range.
        SYMBOL_ADDR_MIN.fetch_min(self.0, Ordering::Release);
        SYMBOL_ADDR_MAX.fetch_max(self.1, Ordering::Release);
    }
}

#[cfg(test)]
pub(crate) fn test_symbol_addr_range() -> (usize, usize) {
    (
        SYMBOL_ADDR_MIN.load(Ordering::Acquire),
        SYMBOL_ADDR_MAX.load(Ordering::Acquire),
    )
}

pub(crate) fn register_symbol_pointer(ptr: usize) {
    // Arm before taking the lock, so the entry is never reachable while the
    // latch still reads idle.
    SYMBOL_EVER_REGISTERED.arm();
    // Widen before the insert, for the same reason.
    widen_symbol_addr_range(ptr);
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    if guard.is_none() {
        *guard = Some(new_ptr_hash_set());
    }
    insert_symbol_pointer_in_set(guard.as_mut().unwrap(), ptr);
}

#[cfg(test)]
thread_local! {
/// Every entry into [`is_registered_symbol`] that got past the latch, i.e.
/// every caller that could not rule a `Symbol` out more cheaply. Twin of
/// `map::TEST_MAP_REGISTRY_PROBES`: #7850's header-directed dispatch in
/// `object::native_call_method` is asserted against this, so "the probe no
/// longer runs on a plain-object dispatch" is a test rather than a claim.
    static TEST_SYMBOL_REGISTRY_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn test_symbol_registry_probe_count() -> u64 {
    TEST_SYMBOL_REGISTRY_PROBES.with(|c| c.get())
}

#[cfg(test)]
pub(crate) fn test_symbol_latch_is_idle() -> bool {
    SYMBOL_EVER_REGISTERED.is_idle()
}

// The `%Intl%.[[FallbackSymbol]]` — a single per-realm symbol whose description
// is exactly `"IntlLegacyConstructedSymbol"` (no `Symbol.` prefix, so it is
// *not* a well-known symbol). It is stashed on the receiver when a legacy Intl
// service constructor (`Intl.NumberFormat` / `Intl.DateTimeFormat`) is called as
// a plain function whose `this` is on the constructor's prototype chain (the
// ChainNumberFormat / ChainDateTimeFormat normative-optional path), and read
// back by the UnwrapXxx step so `nf.resolvedOptions()` on the wrapped object
// still works.
static INTL_FALLBACK_SYMBOL: Mutex<Option<usize>> = Mutex::new(None);

/// Return the process-wide `%Intl%.[[FallbackSymbol]]` as a NaN-boxed
/// (POINTER_TAG) JSValue, allocating & registering it lazily on first use.
pub fn intl_legacy_constructed_symbol() -> f64 {
    let mut guard = INTL_FALLBACK_SYMBOL.lock().unwrap();
    if let Some(ptr) = *guard {
        return f64::from_bits(POINTER_TAG | (ptr as u64 & POINTER_MASK));
    }
    // Persistent (leaked) symbol so it outlives every GC cycle — its identity is
    // realm-global. Description text lives in REGISTERED_SYMBOL_DESCRIPTIONS
    // (readers materialize a fresh StringHeader on demand), matching the
    // well-known-symbol contract.
    let boxed = Box::new(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: 0,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    let sym_ptr = Box::into_raw(boxed) as usize;
    record_registered_symbol_description(sym_ptr, "IntlLegacyConstructedSymbol");
    register_symbol_pointer(sym_ptr);
    *guard = Some(sym_ptr);
    f64::from_bits(POINTER_TAG | (sym_ptr as u64 & POINTER_MASK))
}

/// O(1) check whether a raw pointer (already untagged) is a known Symbol.
/// Safe to call on any pointer-shaped value — no dereference is performed.
#[inline]
pub fn is_registered_symbol(ptr: usize) -> bool {
    // No symbol has ever been allocated ⟹ nothing to find, and in particular no
    // reason to take the process-global registry mutex.
    if SYMBOL_EVER_REGISTERED.is_idle() {
        return false;
    }
    #[cfg(test)]
    TEST_SYMBOL_REGISTRY_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    is_registered_symbol_slow(ptr)
}

/// `PERRY_SYMBOL_RANGE_FILTER=0` restores the unconditional mutex acquisition.
fn symbol_range_filter_enabled() -> bool {
    use std::sync::OnceLock;
    static CACHED: OnceLock<bool> = OnceLock::new();
    *CACHED.get_or_init(|| {
        !matches!(
            std::env::var("PERRY_SYMBOL_RANGE_FILTER").as_deref(),
            Ok("0") | Ok("off") | Ok("false")
        )
    })
}

#[inline(never)]
fn is_registered_symbol_slow(ptr: usize) -> bool {
    if ptr < 0x10000 {
        return false;
    }
    // Outside the registered range ⟹ not a symbol, without the global mutex.
    if symbol_range_filter_enabled()
        && (ptr < SYMBOL_ADDR_MIN.load(Ordering::Acquire)
            || ptr > SYMBOL_ADDR_MAX.load(Ordering::Acquire))
    {
        return false;
    }
    let guard = SYMBOL_POINTERS.lock().unwrap();
    guard.as_ref().is_some_and(|s| s.contains(&ptr))
}

/// True for symbols created through `Symbol.for(...)`. These are known symbols
/// too, but WeakRef / FinalizationRegistry must reject them while accepting
/// fresh and well-known symbols.
pub(crate) fn is_global_registered_symbol(ptr: usize) -> bool {
    if !is_registered_symbol(ptr) {
        return false;
    }
    unsafe {
        let sym = ptr as *const SymbolHeader;
        !sym.is_null() && (*sym).magic == SYMBOL_MAGIC && (*sym).registered != 0
    }
}

// Symbol-keyed property side tables. Object keys are metadata-only and get
// rewritten when owners move; symbol keys and NaN-boxed values are GC roots.
// Storage stays intentionally linear because per-object symbol keys are rare.
per_test_global! {
    static SYMBOL_PROPERTIES: Mutex<Option<PtrHashMap<usize, Vec<(usize, u64)>>>> =
        Mutex::new(None);
}

// Descriptor attributes for symbol-keyed properties installed through
// Object.defineProperty. Direct symbol assignment uses the normal data-property
// defaults, so absence here means writable/enumerable/configurable are all true.
per_test_global! {
    static SYMBOL_PROPERTY_ATTRS: Mutex<
        Option<FastKeyHashMap<(usize, usize), crate::object::PropertyAttrs>>,
    > = Mutex::new(None);
}

/// Death pruning for the symbol-keyed property side tables (2026-07-09 GC
/// audit wave 2). Both tables are PROCESS-global and owner-keyed; the values
/// are strongly rooted by `symbol/gc_roots.rs`, so entries of a dead owner
/// immortalized the whole value graph. `is_dead_owner` is one of the GC's
/// deadness predicates (`gc::dead_owner`), which only attributes THIS
/// thread's heap addresses — entries owned by other threads' objects are
/// skipped (documented residual: cross-thread owners are only reclaimed by
/// the owning thread's own collections; addresses that classify as no-heap
/// are never pruned).
pub(crate) fn prune_dead_symbol_property_owners(is_dead_owner: &dyn Fn(usize) -> bool) {
    let mut verdicts: HashMap<usize, bool> = HashMap::new();
    // #8195: the accessor table is keyed by the SAME owner address and was the
    // one of the three not pruned here. Take its verdicts first, into the same
    // memo, so all three tables agree about every owner within one pass.
    {
        let verdicts = std::cell::RefCell::new(&mut verdicts);
        accessors::prune_dead_symbol_accessor_owners(&|owner| {
            let mut verdicts = verdicts.borrow_mut();
            if let Some(&known) = verdicts.get(&owner) {
                return known;
            }
            let dead = is_dead_owner(owner);
            verdicts.insert(owner, dead);
            dead
        });
    }
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = guard.as_mut() {
            map.retain(|owner, _| {
                !*verdicts
                    .entry(*owner)
                    .or_insert_with(|| is_dead_owner(*owner))
            });
        }
    }
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTY_ATTRS);
        if let Some(map) = guard.as_mut() {
            map.retain(|(owner, _), _| {
                !*verdicts
                    .entry(*owner)
                    .or_insert_with(|| is_dead_owner(*owner))
            });
        }
    }
}

/// Death pruning for `SYMBOL_POINTERS` (2026-07-09 GC audit wave 2): one
/// entry per `Symbol()` ever created, previously only forward-renamed on
/// moves and never removed on death — so the set grew monotonically and
/// `js_is_symbol` aliased later allocations at recycled addresses.
/// `is_dead_symbol` is a `gc::dead_owner` predicate narrowed to
/// `GC_TYPE_STRING` (what `alloc_symbol` gc_malloc's). `Box`-leaked
/// persistent symbols (well-known, `Symbol.for`, the Intl fallback) have no
/// GcHeader and are skipped by the predicate's heap attribution. Residual:
/// a symbol freed by a minor malloc sweep between full traces leaves its
/// entry behind permanently (the address no longer attributes) — fixing
/// that needs a dedicated symbol GC type with a finalize hook.
pub(crate) fn prune_dead_symbol_pointers(is_dead_symbol: &dyn Fn(usize) -> bool) {
    let mut live_ids: Vec<u64> = Vec::new();
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
        if let Some(set) = guard.as_mut() {
            set.retain(|&ptr| !is_dead_symbol(ptr));
            // #7246: the surviving symbols' ids, read while the lock is held and
            // every remaining address is known live. Reading `(*ptr).id` of a
            // symbol the predicate has just rejected would be a read of freed
            // memory, which is why this is a second pass over the RETAINED set
            // rather than a filter inside `retain`.
            live_ids.reserve(set.len());
            for &ptr in set.iter() {
                live_ids.push(unsafe { (*(ptr as *const SymbolHeader)).id });
            }
        }
    }
    // #7246: descriptions are keyed on the id, so they are pruned by the same
    // liveness verdict. Without this a `Symbol("x")` churn loop would retain one
    // `Arc<str>` per symbol for the life of the process — the cost the issue
    // named as this fix's price, paid down here.
    //
    // Only prune when we actually observed a live set: an empty `SYMBOL_POINTERS`
    // (uninitialised registry, or a thread that has allocated no symbols) must
    // not be read as "every description is dead".
    if !live_ids.is_empty() {
        let live: HashSet<u64> = live_ids.into_iter().collect();
        FRESH_SYMBOL_DESCRIPTIONS.with(|m| m.borrow_mut().retain(|id, _| live.contains(id)));
    }
}

// Monotonic id counter for fresh symbols. Not thread-safe per-thread but
// Symbol semantics are compatible with coarse locking.
static NEXT_SYMBOL_ID: Mutex<u64> = Mutex::new(1);

fn next_id() -> u64 {
    let mut id = NEXT_SYMBOL_ID.lock().unwrap();
    let v = *id;
    *id = v.wrapping_add(1);
    v
}

pub(crate) unsafe fn str_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

pub(crate) unsafe fn alloc_symbol(
    description: *mut StringHeader,
    registered: bool,
) -> *mut SymbolHeader {
    // Allocated via gc_malloc as a leaf: `GC_TYPE_STRING`'s type info is
    // `pointer_free: true` / `GcRewriteDescriptorKind::Leaf` /
    // `GcLayoutSlotKind::None`, so nothing walks into the payload.
    //
    // ★ #7246. That was correct for a *string*, whose payload is bytes, and
    // WRONG for a *symbol*, whose payload's third word used to be a
    // `*mut StringHeader`. Symbols and strings share one GC type, so no
    // descriptor could distinguish them and the description was never traced or
    // rewritten: a perfectly rooted symbol could have its description reaped or
    // relocated out from under it, and `String(sym)` / `sym.description` then
    // read recycled memory. `SYMBOL_POINTERS` did not close it either —
    // `scan_symbol_pointer_metadata_roots_mut` uses `visit_metadata_usize_slot`,
    // which rewrites a recorded address WITHOUT marking, and never looks at
    // `(*ptr).description` at all.
    //
    // The pointer is now gone rather than traced. Copy the text off the GC heap
    // BEFORE allocating — so there is never a window in which a description
    // pointer is live-but-untraced — and leave the field null.
    // `FRESH_SYMBOL_DESCRIPTIONS` is keyed on the symbol's `id`, which an
    // evacuation copies verbatim, so that table needs no rekey, no scanner and
    // no budgeted step twin. See its declaration for why this beat a
    // `GC_TYPE_SYMBOL` and beat tracing from the side table.
    //
    // (#7341's `RuntimeHandleScope` + `across_mut` here is therefore gone too:
    // it made the STORED pointer correct across `gc_malloc`, and there is no
    // longer a stored pointer. Nothing is live across the allocation.)
    let description_text = description_bytes_from_header(description);
    let raw = crate::gc::gc_malloc(
        std::mem::size_of::<SymbolHeader>(),
        crate::gc::GC_TYPE_STRING,
    );
    let ptr = raw as *mut SymbolHeader;
    let id = next_id();
    (*ptr).magic = SYMBOL_MAGIC;
    (*ptr).registered = if registered { 1 } else { 0 };
    (*ptr).description = std::ptr::null_mut();
    (*ptr).id = id;
    if let Some(text) = description_text {
        record_fresh_symbol_description(id, &text);
    }
    register_symbol_pointer(ptr as usize);
    ptr
}

/// Check whether a NaN-boxed JSValue is a Symbol.
#[no_mangle]
pub unsafe extern "C" fn js_is_symbol(value: f64) -> i32 {
    let bits = value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != POINTER_TAG {
        return 0;
    }
    let ptr_usize = (bits & POINTER_MASK) as usize;
    if is_registered_symbol(ptr_usize) {
        return 1;
    }
    let ptr = ptr_usize as *const SymbolHeader;
    // Registry handles (proxies, fetch/stream handles, …) are POINTER_TAG'd
    // small ids, NOT heap allocations — dereferencing one for the magic
    // probe segfaults on Linux (unmapped page; mimalloc on macOS happens to
    // retain, hiding it). Real heap symbols live above the handle band
    // (same rationale as the typeof / iterator guards, #1843/#4800), and
    // registered symbols already returned above.
    if crate::value::addr_class::is_handle_band(ptr as usize) {
        return 0;
    }
    if (*ptr).magic == SYMBOL_MAGIC {
        1
    } else {
        0
    }
}

/// Extract the raw object pointer from a NaN-boxed JSValue. Returns 0 if the
/// value isn't a pointer-tagged object (and 0 is also a valid "no entries"
/// sentinel for the side table).
pub(crate) unsafe fn obj_key_from_f64(obj_f64: f64) -> usize {
    let bits = obj_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != POINTER_TAG {
        return 0;
    }
    (bits & POINTER_MASK) as usize
}

/// Extract the raw symbol pointer from a NaN-boxed Symbol JSValue, or 0 if
/// the value isn't a Symbol.
pub(crate) unsafe fn sym_key_from_f64(sym_f64: f64) -> usize {
    let bits = sym_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != POINTER_TAG {
        return 0;
    }
    let ptr = (bits & POINTER_MASK) as *const SymbolHeader;
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return 0;
    }
    if (*ptr).magic != SYMBOL_MAGIC {
        return 0;
    }
    ptr as usize
}

/// Monotonic gate (#6386): has a `Symbol.isConcatSpreadable`-keyed property
/// EVER been installed anywhere (instance symbol store, symbol accessor,
/// symbol defineProperty attrs, class static symbol)? While `false`, the
/// spreadable read `Array.prototype.concat` performs per argument is
/// guaranteed to find undefined for any non-proxy value — and to be
/// side-effect free — so the whole lookup ladder can be skipped.
static CONCAT_SPREADABLE_EVER: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn concat_spreadable_symbol_ever_set() -> bool {
    CONCAT_SPREADABLE_EVER.load(std::sync::atomic::Ordering::Acquire)
}

/// Note a symbol-keyed property install. Flips the gate when the key is the
/// well-known `isConcatSpreadable`. Must be called BEFORE the table insert in
/// every install funnel, so a `false` (acquire) read can never race a
/// completed insert.
pub(crate) fn note_symbol_key_installed(sym_key: usize) {
    if sym_key == 0 || CONCAT_SPREADABLE_EVER.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // Non-allocating peek: a stored key can only BE the well-known
    // `isConcatSpreadable` if that symbol was already materialized (every
    // user route to it goes through `well_known_symbol`). Never create it
    // here — this runs on every symbol install and must not perturb
    // allocation accounting.
    let wk = well_known_symbol_if_cached("isConcatSpreadable");
    if !wk.is_null() && sym_key == wk as usize {
        CONCAT_SPREADABLE_EVER.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// The cached well-known symbol pointer if `short_name` was ever
/// materialized, else null. Unlike [`well_known_symbol`], never allocates.
pub(crate) fn well_known_symbol_if_cached(short_name: &str) -> *mut SymbolHeader {
    let guard = WELL_KNOWN_SYMBOLS.lock().unwrap();
    guard
        .as_ref()
        .and_then(|m| m.get(short_name).copied())
        .unwrap_or(0) as *mut SymbolHeader
}

pub(crate) fn publish_symbol_side_table_root_edges(sym_key: usize, value_bits: u64) {
    crate::gc::runtime_write_barrier_root_raw_ptr(sym_key as *const SymbolHeader);
    crate::gc::runtime_write_barrier_root_nanbox(value_bits);
}

pub(crate) fn store_object_symbol_property_root(
    obj_key: usize,
    sym_key: usize,
    value_bits: u64,
) -> bool {
    note_symbol_key_installed(sym_key);
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if guard.is_none() {
            *guard = Some(new_ptr_hash_map());
        }
        let map = guard.as_mut().unwrap();
        let entries = map.entry(obj_key).or_default();
        for entry in entries.iter_mut() {
            if entry.0 == sym_key {
                entry.1 = value_bits;
                drop(guard);
                publish_symbol_side_table_root_edges(sym_key, value_bits);
                symbol_property_ic_epoch_bump();
                return false;
            }
        }
        entries.push((sym_key, value_bits));
    }
    publish_symbol_side_table_root_edges(sym_key, value_bits);
    symbol_property_ic_epoch_bump();
    true
}

/// Idle until a class declares a static Symbol-keyed member.
///
/// `js_instanceof` consults `CLASS_STATIC_SYMBOLS` for a `Symbol.hasInstance`
/// override on EVERY evaluation, which meant a process-global `Mutex` plus a
/// SipHash probe of an empty map for every `x instanceof C` in a program that
/// never mentions a Symbol (#7769).
pub(crate) static CLASS_STATIC_SYMBOLS_LATCH: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

pub(crate) fn store_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
    note_symbol_key_installed(sym_key);
    CLASS_STATIC_SYMBOLS_LATCH.arm();
    {
        let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        guard
            .as_mut()
            .unwrap()
            .insert((class_id, sym_key), value_bits);
    }
    publish_symbol_side_table_root_edges(sym_key, value_bits);
}

per_test_global! {
    /// Class-id-keyed side table for static Symbol-keyed properties.
    /// drizzle's `static [entityKind] = "Table"` registers
    /// (class_id, sym_ptr) → value here at module init via
    /// `js_class_register_static_symbol`. Consulted by `js_object_has_own`
    /// when the receiver is a class identifier (NaN-boxed INT32_TAG).
    /// Refs #420.
    static CLASS_STATIC_SYMBOLS: Mutex<Option<HashMap<(u32, usize), u64>>> = Mutex::new(None);
}

#[cfg(test)]
mod wellknown_desc_tests {
    use super::*;

    #[test]
    fn well_known_symbols_use_qualified_description() {
        // Spec: `Symbol.iterator.description === "Symbol.iterator"` (qualified),
        // which is also what `console.log` / `String(sym)` report.
        for short in [
            "iterator",
            "asyncIterator",
            "hasInstance",
            "toStringTag",
            "species",
            "match",
            "matchAll",
            "replace",
            "search",
            "split",
            "isConcatSpreadable",
            "unscopables",
            "dispose",
            "asyncDispose",
            "toPrimitive",
        ] {
            let ptr = well_known_symbol(short) as usize;
            let desc = registered_symbol_description(ptr);
            assert_eq!(
                desc.as_deref(),
                Some(format!("Symbol.{short}").as_str()),
                "well-known symbol {short} should have qualified description"
            );
        }
    }
}
