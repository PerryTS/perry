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
    get_symbol_property_attrs, inspect_custom_symbol_ptr, js_object_define_symbol_accessor,
    js_object_delete_symbol_property, js_object_has_own_symbol_property,
    reflect_symbol_getter_closure_bits, set_symbol_property_attrs, symbol_accessor_descriptor_bits,
    symbol_property_is_enumerable, symbol_property_is_non_writable, symbol_property_root_bits,
};
pub use properties::{
    class_static_symbol_lookup, js_class_register_static_symbol, js_object_has_own_symbol,
    js_object_literal_infer_computed_function_name, js_object_set_method_by_name,
    js_object_set_symbol_method, js_object_set_symbol_property,
};

// Symbol-keyed property reads.
pub use get::js_object_get_symbol_property;
pub(crate) use get::{inherited_symbol_property, own_symbol_property};

// Iterator protocol, getOwnPropertySymbols, ToPrimitive.
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

use crate::string::{js_string_from_bytes, StringHeader};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

// NaN-boxing tags (must match value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

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
static SYMBOL_POINTERS: Mutex<Option<HashSet<usize>>> = Mutex::new(None);

/// Process-lifetime descriptions for registered (`Symbol.for`) and well-known
/// symbols. These symbols are Box-leaked so they outlive every GC cycle, but
/// the description StringHeader they used to point at was allocated in the
/// calling thread's arena — which gets freed when a `perry/thread` worker
/// exits, leaving the symbol with a dangling description pointer. Storing
/// the description text here (Rust-owned, process-lifetime) lets readers
/// materialize a fresh StringHeader in the *caller's* arena on demand, which
/// is the only thread-safe contract: the symbol identity is global, but
/// every StringHeader belongs to exactly one thread's arena.
static REGISTERED_SYMBOL_DESCRIPTIONS: Mutex<Option<HashMap<usize, std::sync::Arc<str>>>> =
    Mutex::new(None);

pub(crate) fn registered_symbol_description(sym_ptr: usize) -> Option<std::sync::Arc<str>> {
    let guard = REGISTERED_SYMBOL_DESCRIPTIONS.lock().unwrap();
    guard.as_ref().and_then(|m| m.get(&sym_ptr).cloned())
}

pub(crate) fn record_registered_symbol_description(sym_ptr: usize, description: &str) {
    let mut guard = REGISTERED_SYMBOL_DESCRIPTIONS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
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

pub(crate) fn register_symbol_pointer(ptr: usize) {
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    if guard.is_none() {
        *guard = Some(HashSet::new());
    }
    guard.as_mut().unwrap().insert(ptr);
}

/// O(1) check whether a raw pointer (already untagged) is a known Symbol.
/// Safe to call on any pointer-shaped value — no dereference is performed.
pub fn is_registered_symbol(ptr: usize) -> bool {
    if ptr < 0x10000 {
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
static SYMBOL_PROPERTIES: Mutex<Option<HashMap<usize, Vec<(usize, u64)>>>> = Mutex::new(None);

// Descriptor attributes for symbol-keyed properties installed through
// Object.defineProperty. Direct symbol assignment uses the normal data-property
// defaults, so absence here means writable/enumerable/configurable are all true.
static SYMBOL_PROPERTY_ATTRS: Mutex<Option<HashMap<(usize, usize), crate::object::PropertyAttrs>>> =
    Mutex::new(None);

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
    // Allocate via gc_malloc as a leaf (GC_TYPE_STRING treats payload as
    // opaque, which is what we want — the GC won't try to scan internal
    // pointers). The description pointer is kept alive through the
    // SYMBOL_REGISTRY (for registered symbols) or not at all (for fresh
    // symbols — in practice they live for the duration of the program,
    // which is fine for test workloads).
    let raw = crate::gc::gc_malloc(
        std::mem::size_of::<SymbolHeader>(),
        crate::gc::GC_TYPE_STRING,
    );
    let ptr = raw as *mut SymbolHeader;
    (*ptr).magic = SYMBOL_MAGIC;
    (*ptr).registered = if registered { 1 } else { 0 };
    (*ptr).description = description;
    (*ptr).id = next_id();
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

pub(crate) fn publish_symbol_side_table_root_edges(sym_key: usize, value_bits: u64) {
    crate::gc::runtime_write_barrier_root_raw_ptr(sym_key as *const SymbolHeader);
    crate::gc::runtime_write_barrier_root_nanbox(value_bits);
}

pub(crate) fn store_object_symbol_property_root(
    obj_key: usize,
    sym_key: usize,
    value_bits: u64,
) -> bool {
    {
        let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if guard.is_none() {
            *guard = Some(HashMap::new());
        }
        let map = guard.as_mut().unwrap();
        let entries = map.entry(obj_key).or_default();
        for entry in entries.iter_mut() {
            if entry.0 == sym_key {
                entry.1 = value_bits;
                drop(guard);
                publish_symbol_side_table_root_edges(sym_key, value_bits);
                return false;
            }
        }
        entries.push((sym_key, value_bits));
    }
    publish_symbol_side_table_root_edges(sym_key, value_bits);
    true
}

pub(crate) fn store_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
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

/// Class-id-keyed side table for static Symbol-keyed properties.
/// drizzle's `static [entityKind] = "Table"` registers
/// (class_id, sym_ptr) → value here at module init via
/// `js_class_register_static_symbol`. Consulted by `js_object_has_own`
/// when the receiver is a class identifier (NaN-boxed INT32_TAG).
/// Refs #420.
static CLASS_STATIC_SYMBOLS: Mutex<Option<HashMap<(u32, usize), u64>>> = Mutex::new(None);

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
