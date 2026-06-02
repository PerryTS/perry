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

fn record_registered_symbol_description(sym_ptr: usize, description: &str) {
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

fn register_symbol_pointer(ptr: usize) {
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

// Side-table for symbol-keyed properties on objects. The object pointer is
// the key (as usize); the value is a list of (symbol_ptr, value_bits) pairs.
// Storage is intentionally simple (linear scan per lookup) — symbol-keyed
// properties on a single object are rare.
//
// GC invariant:
// - SYMBOL_PROPERTIES outer object keys are metadata-only raw pointers. They
//   are rewritten when an owner moves but do not keep that owner alive.
// - SYMBOL_PROPERTIES inner symbol keys are raw-pointer roots.
// - SYMBOL_PROPERTIES values and CLASS_STATIC_SYMBOLS values are NaN-box roots.
// - CLASS_STATIC_SYMBOLS symbol keys are raw-pointer roots.
// - SYMBOL_POINTERS is metadata-only: moved symbol addresses are rewritten, but
//   tracking a pointer there does not by itself keep the symbol alive.
static SYMBOL_PROPERTIES: Mutex<Option<HashMap<usize, Vec<(usize, u64)>>>> = Mutex::new(None);

// Monotonic id counter for fresh symbols. Not thread-safe per-thread but
// Symbol semantics are compatible with coarse locking.
static NEXT_SYMBOL_ID: Mutex<u64> = Mutex::new(1);

fn next_id() -> u64 {
    let mut id = NEXT_SYMBOL_ID.lock().unwrap();
    let v = *id;
    *id = v.wrapping_add(1);
    v
}

unsafe fn str_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    let len = (*ptr).byte_len as usize;
    let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
    let bytes = std::slice::from_raw_parts(data, len);
    std::str::from_utf8(bytes).ok().map(|s| s.to_string())
}

unsafe fn alloc_symbol(description: *mut StringHeader, registered: bool) -> *mut SymbolHeader {
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
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return 0;
    }
    if (*ptr).magic == SYMBOL_MAGIC {
        1
    } else {
        0
    }
}

/// `Symbol()` with no description — allocates a fresh unique symbol.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_new_empty() -> f64 {
    let sym = alloc_symbol(std::ptr::null_mut(), false);
    f64::from_bits(POINTER_TAG | (sym as u64 & POINTER_MASK))
}

/// `Symbol(description)` — allocates a fresh unique symbol with description.
/// `description_f64` is a NaN-boxed string JSValue.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_new(description_f64: f64) -> f64 {
    let bits = description_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    let desc_ptr: *mut StringHeader = if tag == STRING_TAG {
        (bits & POINTER_MASK) as *mut StringHeader
    } else if bits == TAG_UNDEFINED {
        std::ptr::null_mut()
    } else {
        // Try to coerce — if it's a raw pointer, trust it.
        if (0x1000..0x0000_FFFF_FFFF_FFFF).contains(&bits) {
            bits as *mut StringHeader
        } else {
            std::ptr::null_mut()
        }
    };
    let sym = alloc_symbol(desc_ptr, false);
    f64::from_bits(POINTER_TAG | (sym as u64 & POINTER_MASK))
}

/// `Symbol.for(key)` — look up the global registry and return the existing
/// symbol, or create and register a new one.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_for(key_f64: f64) -> f64 {
    let bits = key_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    let key_ptr = if tag == STRING_TAG {
        (bits & POINTER_MASK) as *const StringHeader
    } else if (0x1000..0x0000_FFFF_FFFF_FFFF).contains(&bits) {
        bits as *const StringHeader
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let key = match str_from_header(key_ptr) {
        Some(s) => s,
        None => return f64::from_bits(TAG_UNDEFINED),
    };

    // Well-known symbol sentinel: HIR lowers `Symbol.toPrimitive` etc. to
    // `SymbolFor(String("@@__perry_wk_toPrimitive"))`. Detect the prefix
    // and delegate to the well-known cache instead of polluting the
    // Symbol.for registry. These symbols have `registered=0` so
    // `Symbol.keyFor()` returns undefined for them.
    if let Some(short_name) = key.strip_prefix(WK_PREFIX) {
        let wk_ptr = well_known_symbol(short_name);
        return f64::from_bits(POINTER_TAG | (wk_ptr as u64 & POINTER_MASK));
    }

    let mut guard = SYMBOL_REGISTRY.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let registry = guard.as_mut().unwrap();
    if let Some(&ptr_usize) = registry.get(&key) {
        return f64::from_bits(POINTER_TAG | (ptr_usize as u64 & POINTER_MASK));
    }

    // Not found — allocate a persistent SymbolHeader. We use Box::leak so the
    // pointer outlives any GC cycle (the registry holds it as a root). The
    // description text is stored in REGISTERED_SYMBOL_DESCRIPTIONS as a
    // process-lifetime Arc<str>; the header's `description` pointer stays
    // null. Readers (`sym.description`, `sym.toString()`, key_for) consult
    // the side table and materialize a StringHeader in *their own* arena on
    // demand, so cross-thread reads are safe even when the originating
    // worker's arena was torn down.
    let boxed = Box::new(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: 1,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    let sym_ptr = Box::into_raw(boxed);
    // Fully initialize the side tables BEFORE publishing the pointer in
    // the registry. Otherwise a concurrent `Symbol.for("same_key")` on
    // another thread can see the pointer via the registry but get None
    // from registered_symbol_description, returning a transiently bogus
    // sym.description / sym.toString() / Symbol.keyFor(). Lock order is
    // SYMBOL_REGISTRY → SYMBOL_POINTERS → REGISTERED_SYMBOL_DESCRIPTIONS;
    // no reader takes them in the reverse order.
    record_registered_symbol_description(sym_ptr as usize, &key);
    register_symbol_pointer(sym_ptr as usize);
    registry.insert(key.clone(), sym_ptr as usize);
    drop(guard);
    f64::from_bits(POINTER_TAG | (sym_ptr as u64 & POINTER_MASK))
}

/// `Symbol.keyFor(sym)` — reverse lookup. Returns the registration key as a
/// string for registered symbols, or undefined for non-registered symbols.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_key_for(sym_f64: f64) -> f64 {
    let bits = sym_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    let sym_ptr = if tag == POINTER_TAG {
        (bits & POINTER_MASK) as *const SymbolHeader
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if sym_ptr.is_null() || (sym_ptr as usize) < 0x1000 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if (*sym_ptr).magic != SYMBOL_MAGIC {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Well-known symbols (Symbol.toPrimitive, etc.) are NOT in the registry.
    if is_well_known_symbol(sym_ptr as usize) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if (*sym_ptr).registered == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Registered symbols carry the description as Arc<str> in the side
    // table; materialize a fresh StringHeader in this thread's arena.
    if let Some(s) = registered_symbol_description(sym_ptr as usize) {
        let header = js_string_from_bytes(s.as_bytes().as_ptr(), s.as_bytes().len() as u32);
        return f64::from_bits(STRING_TAG | (header as u64 & POINTER_MASK));
    }
    let desc = (*sym_ptr).description;
    if desc.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    f64::from_bits(STRING_TAG | (desc as u64 & POINTER_MASK))
}

/// `sym.description` — returns the original description or undefined.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_description(sym_f64: f64) -> f64 {
    let bits = sym_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    let sym_ptr = if tag == POINTER_TAG {
        (bits & POINTER_MASK) as *const SymbolHeader
    } else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    if sym_ptr.is_null() || (sym_ptr as usize) < 0x1000 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if (*sym_ptr).magic != SYMBOL_MAGIC {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if let Some(s) = registered_symbol_description(sym_ptr as usize) {
        let header = js_string_from_bytes(s.as_bytes().as_ptr(), s.as_bytes().len() as u32);
        return f64::from_bits(STRING_TAG | (header as u64 & POINTER_MASK));
    }
    let desc = (*sym_ptr).description;
    if desc.is_null() {
        return f64::from_bits(TAG_UNDEFINED);
    }
    f64::from_bits(STRING_TAG | (desc as u64 & POINTER_MASK))
}

/// `sym.toString()` — returns "Symbol(description)" as a StringHeader pointer.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_to_string(sym_f64: f64) -> i64 {
    let bits = sym_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    let sym_ptr = if tag == POINTER_TAG {
        (bits & POINTER_MASK) as *const SymbolHeader
    } else {
        let s = b"Symbol()";
        return js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64;
    };
    if sym_ptr.is_null() || (sym_ptr as usize) < 0x1000 || (*sym_ptr).magic != SYMBOL_MAGIC {
        let s = b"Symbol()";
        return js_string_from_bytes(s.as_ptr(), s.len() as u32) as i64;
    }
    let desc_str = if let Some(s) = registered_symbol_description(sym_ptr as usize) {
        s.as_ref().to_string()
    } else {
        str_from_header((*sym_ptr).description).unwrap_or_default()
    };
    let rendered = format!("Symbol({})", desc_str);
    js_string_from_bytes(rendered.as_ptr(), rendered.len() as u32) as i64
}

/// Snapshot the symbol-keyed properties of `src_obj_ptr` (raw object pointer,
/// NOT NaN-boxed). Returns a freshly cloned `Vec<(sym_ptr, value_bits)>` so
/// callers can iterate without holding the SYMBOL_PROPERTIES lock — important
/// when each iteration may itself need to take the same lock (e.g.
/// `Object.assign(target, source)` re-entering `js_object_set_symbol_property`).
/// Look up the cached pointer for the registered `util.inspect.custom` symbol
/// (description `"nodejs.util.inspect.custom"`). Returns 0 if the symbol has
/// not been allocated yet — which means no user code has touched
/// `util.inspect.custom` so no object can possibly hold it as a key.
/// Used by the inspect formatter to detect the hook without iterating every
/// symbol entry. Refs #1201.
pub(crate) fn inspect_custom_symbol_ptr() -> usize {
    let guard = SYMBOL_REGISTRY.lock().unwrap();
    if let Some(map) = guard.as_ref() {
        if let Some(&ptr) = map.get("nodejs.util.inspect.custom") {
            return ptr;
        }
    }
    0
}

pub(crate) fn clone_symbol_entries_for_obj_ptr(src_obj_ptr: usize) -> Vec<(usize, u64)> {
    if src_obj_ptr == 0 {
        return Vec::new();
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard
        .as_ref()
        .and_then(|m| m.get(&src_obj_ptr))
        .cloned()
        .unwrap_or_default()
}

/// Extract the raw object pointer from a NaN-boxed JSValue. Returns 0 if the
/// value isn't a pointer-tagged object (and 0 is also a valid "no entries"
/// sentinel for the side table).
unsafe fn obj_key_from_f64(obj_f64: f64) -> usize {
    let bits = obj_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != POINTER_TAG {
        return 0;
    }
    (bits & POINTER_MASK) as usize
}

/// Extract the raw symbol pointer from a NaN-boxed Symbol JSValue, or 0 if
/// the value isn't a Symbol.
unsafe fn sym_key_from_f64(sym_f64: f64) -> usize {
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

unsafe fn infer_symbol_function_name(sym_key: usize, val_bits: u64) {
    let val_tag = val_bits & 0xFFFF_0000_0000_0000;
    if val_tag != POINTER_TAG {
        return;
    }
    let val_ptr = (val_bits & POINTER_MASK) as *const u8;
    if val_ptr.is_null() || (val_ptr as usize) <= 0x10000 {
        return;
    }
    let gc_header = val_ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_CLOSURE {
        return;
    }
    let closure_ptr = val_ptr as *const crate::closure::ClosureHeader;
    let func_ptr = (*closure_ptr).func_ptr;
    if func_ptr.is_null() {
        return;
    }
    let sym_ptr = sym_key as *const SymbolHeader;
    let desc = registered_symbol_description(sym_ptr as usize)
        .map(|s| s.as_ref().to_string())
        .unwrap_or_else(|| str_from_header((*sym_ptr).description).unwrap_or_default());
    let inferred = format!("[{}]", desc);
    crate::builtins::register_function_name_if_absent(func_ptr as usize, &inferred);
}

fn publish_symbol_side_table_root_edges(sym_key: usize, value_bits: u64) {
    crate::gc::runtime_write_barrier_root_raw_ptr(sym_key as *const SymbolHeader);
    crate::gc::runtime_write_barrier_root_nanbox(value_bits);
}

fn store_object_symbol_property_root(obj_key: usize, sym_key: usize, value_bits: u64) -> bool {
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

fn store_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
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

unsafe fn set_symbol_property(obj_f64: f64, sym_f64: f64, value_f64: f64) -> f64 {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return value_f64;
    }
    store_object_symbol_property_root(obj_key, sym_key, value_f64.to_bits());
    value_f64
}

/// `obj[sym] = value` where `sym` is a Symbol. Stores into the side table.
/// Returns the value (NaN-boxed) for chained assignment semantics.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_symbol_property(
    obj_f64: f64,
    sym_f64: f64,
    value_f64: f64,
) -> f64 {
    set_symbol_property(obj_f64, sym_f64, value_f64)
}

/// Computed-key object literal function-name inference. Storage stays on the
/// normal IndexSet path, but object literals get Node's `[symbol.description]`
/// name for anonymous functions assigned under symbol keys.
#[no_mangle]
pub unsafe extern "C" fn js_object_literal_infer_computed_function_name(
    key_f64: f64,
    value_f64: f64,
) -> f64 {
    let sym_key = sym_key_from_f64(key_f64);
    if sym_key != 0 {
        infer_symbol_function_name(sym_key, value_f64.to_bits());
    }
    value_f64
}

unsafe fn js_object_set_symbol_property_infer_name(
    obj_f64: f64,
    sym_f64: f64,
    value_f64: f64,
) -> f64 {
    let stored = set_symbol_property(obj_f64, sym_f64, value_f64);
    js_object_literal_infer_computed_function_name(sym_f64, value_f64);
    stored
}

/// Class-id-keyed side table for static Symbol-keyed properties.
/// drizzle's `static [entityKind] = "Table"` registers
/// (class_id, sym_ptr) → value here at module init via
/// `js_class_register_static_symbol`. Consulted by `js_object_has_own`
/// when the receiver is a class identifier (NaN-boxed INT32_TAG).
/// Refs #420.
static CLASS_STATIC_SYMBOLS: Mutex<Option<HashMap<(u32, usize), u64>>> = Mutex::new(None);

/// Register a static Symbol-keyed field on a class. Called once per
/// class + static computed-key field at module init.
#[no_mangle]
pub unsafe extern "C" fn js_class_register_static_symbol(class_id: u32, sym: f64, value: f64) {
    let sym_key = sym_key_from_f64(sym);
    if class_id == 0 || sym_key == 0 {
        return;
    }
    store_class_static_symbol_root(class_id, sym_key, value.to_bits());
}

/// Look up a static Symbol-keyed property on a class by class_id.
/// Returns the stored value bits or `None` if no entry. Refs #420.
pub fn class_static_symbol_lookup(class_id: u32, sym_f64: f64) -> Option<u64> {
    unsafe {
        let sym_key = sym_key_from_f64(sym_f64);
        if class_id == 0 || sym_key == 0 {
            return None;
        }
        let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        guard
            .as_ref()
            .and_then(|m| m.get(&(class_id, sym_key)).copied())
    }
}

fn merge_symbol_property_entries(dst: &mut Vec<(usize, u64)>, src: Vec<(usize, u64)>) {
    for (sym_key, value_bits) in src {
        if let Some(existing) = dst.iter_mut().find(|entry| entry.0 == sym_key) {
            existing.1 = value_bits;
        } else {
            dst.push((sym_key, value_bits));
        }
    }
}

pub fn scan_symbol_side_table_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_symbol_side_table_roots_mut(&mut visitor);
}

pub fn scan_symbol_side_table_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    scan_symbol_property_roots_mut(visitor);
    scan_class_static_symbol_roots_mut(visitor);
    scan_symbol_pointer_metadata_roots_mut(visitor);
}

fn scan_symbol_property_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut owner_rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for (&owner, entries) in map.iter_mut() {
        let mut new_owner = owner;
        if visitor.visit_metadata_usize_slot(&mut new_owner) && new_owner != owner {
            owner_rewrites.push((owner, new_owner));
        }
        for (sym_key, value_bits) in entries.iter_mut() {
            visitor.visit_usize_slot(sym_key);
            visitor.visit_nanbox_u64_slot(value_bits);
        }
    }

    for (old_owner, new_owner) in owner_rewrites {
        let Some(entries) = map.remove(&old_owner) else {
            continue;
        };
        match map.entry(new_owner) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                merge_symbol_property_entries(entry.get_mut(), entries);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(entries);
            }
        }
    }
}

fn scan_class_static_symbol_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut key_rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    let Some(map) = guard.as_mut() else {
        return;
    };

    for (class_id, old_sym_key) in map.keys().copied().collect::<Vec<_>>() {
        let Some(value_bits) = map.get_mut(&(class_id, old_sym_key)) else {
            continue;
        };
        let mut new_sym_key = old_sym_key;
        if visitor.visit_usize_slot(&mut new_sym_key) && new_sym_key != old_sym_key {
            key_rewrites.push(((class_id, old_sym_key), (class_id, new_sym_key)));
        }
        visitor.visit_nanbox_u64_slot(value_bits);
    }

    for (old_key, new_key) in key_rewrites {
        if let Some(value_bits) = map.remove(&old_key) {
            map.insert(new_key, value_bits);
        }
    }
}

fn scan_symbol_pointer_metadata_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    let mut rewrites = Vec::new();
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    let Some(set) = guard.as_mut() else {
        return;
    };
    for old_ptr in set.iter().copied().collect::<Vec<_>>() {
        let mut new_ptr = old_ptr;
        if visitor.visit_metadata_usize_slot(&mut new_ptr) && new_ptr != old_ptr {
            rewrites.push((old_ptr, new_ptr));
        }
    }
    for (old_ptr, new_ptr) in rewrites {
        set.remove(&old_ptr);
        if new_ptr != 0 {
            set.insert(new_ptr);
        }
    }
}

#[derive(Clone, Copy)]
enum SymbolSideTableRootSlot {
    SymbolPropertyOwner { owner: usize },
    SymbolPropertyEntry { owner: usize, sym_key: usize },
    ClassStaticSymbol { class_id: u32, sym_key: usize },
    SymbolPointer { ptr: usize },
}

pub(crate) struct SymbolSideTableRootScanState {
    slots: Vec<SymbolSideTableRootSlot>,
    cursor: usize,
}

pub(crate) fn new_symbol_side_table_root_scan_state() -> Box<dyn std::any::Any> {
    Box::new(SymbolSideTableRootScanState {
        slots: symbol_side_table_root_snapshot(),
        cursor: 0,
    })
}

pub(crate) fn scan_symbol_side_table_roots_mut_step(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    state: &mut dyn std::any::Any,
    remaining: &mut usize,
) -> bool {
    let state = state
        .downcast_mut::<SymbolSideTableRootScanState>()
        .expect("symbol side-table root scanner state type");
    while *remaining > 0 && state.cursor < state.slots.len() {
        scan_symbol_side_table_root_slot(visitor, state.slots[state.cursor]);
        state.cursor += 1;
        *remaining -= 1;
    }
    state.cursor >= state.slots.len()
}

fn symbol_side_table_root_snapshot() -> Vec<SymbolSideTableRootSlot> {
    let mut slots = Vec::new();

    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
        if let Some(map) = guard.as_ref() {
            for (&owner, entries) in map.iter() {
                slots.push(SymbolSideTableRootSlot::SymbolPropertyOwner { owner });
                for &(sym_key, _) in entries.iter() {
                    slots.push(SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key });
                }
            }
        }
    }

    {
        let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
        if let Some(map) = guard.as_ref() {
            for &(class_id, sym_key) in map.keys() {
                slots.push(SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key });
            }
        }
    }

    {
        let guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
        if let Some(set) = guard.as_ref() {
            for &ptr in set.iter() {
                slots.push(SymbolSideTableRootSlot::SymbolPointer { ptr });
            }
        }
    }

    slots
}

fn scan_symbol_side_table_root_slot(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    slot: SymbolSideTableRootSlot,
) {
    match slot {
        SymbolSideTableRootSlot::SymbolPropertyOwner { owner } => {
            rewrite_symbol_property_owner_if_forwarded(visitor, owner);
        }
        SymbolSideTableRootSlot::SymbolPropertyEntry { owner, sym_key } => {
            let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
            let Some((entry_sym, value_bits)) = guard
                .as_mut()
                .and_then(|map| map.get_mut(&owner))
                .and_then(|entries| entries.iter_mut().find(|entry| entry.0 == sym_key))
            else {
                return;
            };
            visitor.visit_usize_slot(entry_sym);
            visitor.visit_nanbox_u64_slot(value_bits);
        }
        SymbolSideTableRootSlot::ClassStaticSymbol { class_id, sym_key } => {
            rewrite_class_static_symbol_entry_if_forwarded(visitor, class_id, sym_key);
        }
        SymbolSideTableRootSlot::SymbolPointer { ptr } => {
            rewrite_symbol_pointer_metadata_if_forwarded(visitor, ptr);
        }
    }
}

fn rewrite_symbol_property_owner_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    owner: usize,
) {
    let mut new_owner = owner;
    if !visitor.visit_metadata_usize_slot(&mut new_owner) || new_owner == owner {
        return;
    }
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_mut() {
        if let Some(entries) = map.remove(&owner) {
            match map.entry(new_owner) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    merge_symbol_property_entries(entry.get_mut(), entries);
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(entries);
                }
            }
        }
    }
}

fn rewrite_class_static_symbol_entry_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    class_id: u32,
    sym_key: usize,
) {
    let mut guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    let Some(map) = guard.as_mut() else {
        return;
    };
    let Some(value_bits) = map.get_mut(&(class_id, sym_key)) else {
        return;
    };
    let mut new_sym_key = sym_key;
    let moved = visitor.visit_usize_slot(&mut new_sym_key);
    visitor.visit_nanbox_u64_slot(value_bits);
    if moved && new_sym_key != sym_key {
        if let Some(value_bits) = map.remove(&(class_id, sym_key)) {
            map.insert((class_id, new_sym_key), value_bits);
        }
    }
}

fn rewrite_symbol_pointer_metadata_if_forwarded(
    visitor: &mut crate::gc::RuntimeRootVisitor<'_>,
    ptr: usize,
) {
    let mut new_ptr = ptr;
    if !visitor.visit_metadata_usize_slot(&mut new_ptr) || new_ptr == ptr {
        return;
    }
    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    if let Some(set) = guard.as_mut() {
        set.remove(&ptr);
        if new_ptr != 0 {
            set.insert(new_ptr);
        }
    }
}

#[cfg(test)]
pub(crate) fn test_clear_symbol_side_table_roots() {
    *crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES) = None;
    *crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS) = None;

    let mut persistent = Vec::new();
    {
        let guard = SYMBOL_REGISTRY.lock().unwrap();
        if let Some(map) = guard.as_ref() {
            persistent.extend(map.values().copied());
        }
    }
    {
        let guard = WELL_KNOWN_SYMBOLS.lock().unwrap();
        if let Some(map) = guard.as_ref() {
            persistent.extend(map.values().copied());
        }
    }

    let mut guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    if persistent.is_empty() {
        *guard = None;
    } else {
        *guard = Some(persistent.into_iter().collect());
    }
}

#[cfg(test)]
pub(crate) fn test_seed_symbol_property_root(owner: usize, sym_key: usize, value_bits: u64) {
    if owner != 0 && sym_key != 0 {
        store_object_symbol_property_root(owner, sym_key, value_bits);
    }
}

#[cfg(test)]
pub(crate) fn test_symbol_property_roots(owner: usize) -> Vec<(usize, u64)> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard
        .as_ref()
        .and_then(|map| map.get(&owner))
        .cloned()
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn test_symbol_property_root_bits(owner: usize, sym_key: usize) -> Option<u64> {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard.as_ref().and_then(|map| {
        map.get(&owner)
            .and_then(|entries| entries.iter().find(|entry| entry.0 == sym_key))
            .map(|entry| entry.1)
    })
}

#[cfg(test)]
pub(crate) fn test_symbol_property_owner_exists(owner: usize) -> bool {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    guard.as_ref().is_some_and(|map| map.contains_key(&owner))
}

#[cfg(test)]
pub(crate) fn test_seed_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
    if class_id != 0 && sym_key != 0 {
        store_class_static_symbol_root(class_id, sym_key, value_bits);
    }
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_root_bits(class_id: u32, sym_key: usize) -> Option<u64> {
    let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    guard
        .as_ref()
        .and_then(|map| map.get(&(class_id, sym_key)).copied())
}

#[cfg(test)]
pub(crate) fn test_class_static_symbol_roots_for_class(class_id: u32) -> Vec<(usize, u64)> {
    let guard = crate::gc::lock_gc_root_registry(&CLASS_STATIC_SYMBOLS);
    guard
        .as_ref()
        .map(|map| {
            map.iter()
                .filter_map(|(&(cid, sym_key), &value_bits)| {
                    (cid == class_id).then_some((sym_key, value_bits))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn test_seed_symbol_pointer_root(ptr: usize) {
    if ptr != 0 {
        register_symbol_pointer(ptr);
    }
}

#[cfg(test)]
pub(crate) fn test_symbol_pointer_root_contains(ptr: usize) -> bool {
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_POINTERS);
    guard.as_ref().is_some_and(|set| set.contains(&ptr))
}

/// `Object.prototype.hasOwnProperty.call(obj, sym)` for Symbol keys.
/// Refs #420 — drizzle's `is(value, type)` checks entityKind which is a Symbol.
///
/// When `obj` is an INT32-tagged class ref, also consult
/// `CLASS_STATIC_SYMBOLS` for static-Symbol-keyed declarations.
#[no_mangle]
pub unsafe extern "C" fn js_object_has_own_symbol(obj_f64: f64, sym_f64: f64) -> bool {
    let bits = obj_f64.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        return class_static_symbol_lookup(class_id, sym_f64).is_some();
    }
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return false;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_ref() {
        if let Some(entries) = map.get(&obj_key) {
            for &(sk, _) in entries.iter() {
                if sk == sym_key {
                    return true;
                }
            }
        }
    }
    false
}

/// `obj[sym]` where `sym` is a Symbol. Returns NaN-boxed undefined if the
/// property isn't present.
///
/// Refs #420: when `obj` is an INT32-tagged class ref (drizzle's
/// `cls[entityKind]` chain), also consult `CLASS_STATIC_SYMBOLS` —
/// `static [Symbol] = X` declarations are registered there at module
/// init via `js_class_register_static_symbol`. Pre-fix the dispatch
/// only looked at the per-instance `SYMBOL_PROPERTIES` map and class
/// refs always returned undefined.
/// #1758: the OWN symbol-property lookup — the raw `SYMBOL_PROPERTIES`
/// side-table read keyed by the object's address (no class-ref / no prototype
/// chain). Used by `js_object_get_symbol_property` and by
/// `resolve_proto_chain_symbol`, which walks prototype objects itself and must
/// therefore NOT recurse into the full chain-walking getter.
pub(crate) unsafe fn own_symbol_property(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    let obj_key = obj_key_from_f64(obj_f64);
    let sym_key = sym_key_from_f64(sym_f64);
    if obj_key == 0 || sym_key == 0 {
        return None;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    if let Some(map) = guard.as_ref() {
        if let Some(entries) = map.get(&obj_key) {
            for &(sk, vb) in entries.iter() {
                if sk == sym_key {
                    return Some(f64::from_bits(vb));
                }
            }
        }
    }
    None
}

unsafe fn object_header_ptr_from_value_bits(bits: u64) -> Option<usize> {
    let top16 = bits >> 48;
    let raw = if top16 == 0x7FFD {
        (bits & POINTER_MASK) as usize
    } else if top16 == 0 {
        bits as usize
    } else {
        return None;
    };
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header_addr = raw - crate::gc::GC_HEADER_SIZE;
    let gc_header = header_addr as *const crate::gc::GcHeader;
    let tracked_malloc = crate::gc::gc_malloc_header_is_tracked(gc_header);
    let arena_payload = !matches!(
        crate::arena::classify_heap_space(raw),
        crate::arena::HeapSpace::Unknown
    );
    let arena_header = !matches!(
        crate::arena::classify_heap_space(header_addr),
        crate::arena::HeapSpace::Unknown
    );
    if !tracked_malloc && !(arena_payload && arena_header) {
        return None;
    }
    if (*gc_header).obj_type == crate::gc::GC_TYPE_OBJECT {
        Some(raw)
    } else {
        None
    }
}

unsafe fn resolve_explicit_object_prototype_symbol(obj_f64: f64, sym_f64: f64) -> Option<f64> {
    const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;
    let mut owner = object_header_ptr_from_value_bits(obj_f64.to_bits())?;
    for _ in 0..8 {
        let proto_bits = crate::object::prototype_chain::object_static_prototype(owner)?;
        if proto_bits == TAG_NULL {
            return None;
        }
        let proto_f64 = f64::from_bits(proto_bits);
        if let Some(v) = own_symbol_property(proto_f64, sym_f64) {
            return Some(v);
        }
        let proto_ptr = object_header_ptr_from_value_bits(proto_bits)?;
        if proto_ptr == owner {
            return None;
        }
        let proto_obj = proto_ptr as *const crate::object::ObjectHeader;
        let cid = crate::object::js_object_get_class_id(proto_obj);
        if cid != 0 {
            if let Some(v) = crate::object::resolve_proto_chain_symbol(cid, sym_f64) {
                return Some(v);
            }
        }
        owner = proto_ptr;
    }
    None
}

#[no_mangle]
pub unsafe extern "C" fn js_object_get_symbol_property(obj_f64: f64, sym_f64: f64) -> f64 {
    // Check CLASS_STATIC_SYMBOLS first when receiver is a class ref
    // (top16 == 0x7FFE, INT32_TAG).
    let bits = obj_f64.to_bits();
    if (bits >> 48) == 0x7FFE {
        let class_id = (bits & 0xFFFF_FFFF) as u32;
        if let Some(vb) = class_static_symbol_lookup(class_id, sym_f64) {
            return f64::from_bits(vb);
        }
        // #1758: a class ref whose own static symbols miss may inherit the
        // symbol from a class-expression parent (`class Sub extends make(...) {}`
        // → `Sub[TypeId]`). Walk the CLASS_PROTOTYPE_OBJECTS chain.
        if let Some(v) = crate::object::resolve_proto_chain_symbol(class_id, sym_f64) {
            return v;
        }
        // #36 / #321: the subclass extends a FUNCTION value
        // (`class Svc extends Context.Tag(id)<...>() {}`). Read the symbol off
        // the parent closure — own symbol props plus, via the closure symbol
        // getter, its static prototype (`Svc[TagTypeId]`/`Svc[EffectTypeId]`
        // live on TagProto). Recurse into the closure-aware getter so its proto
        // walk fires.
        if let Some(closure_ptr) = crate::object::class_parent_closure(class_id) {
            let closure_f64 =
                f64::from_bits(crate::value::js_nanbox_pointer(closure_ptr as i64).to_bits());
            let v = js_object_get_symbol_property(closure_f64, sym_f64);
            if v.to_bits() != TAG_UNDEFINED {
                return v;
            }
        }
        return f64::from_bits(TAG_UNDEFINED);
    }
    // #1213: Timeout/Immediate handles expose `Symbol.dispose` so
    // `using t = setTimeout(...)` and `t[Symbol.dispose]()` clear the timer.
    // The handle is a small id NaN-boxed as POINTER; the symbol-keyed read
    // otherwise misses the side table and returns undefined.
    if (bits >> 48) == 0x7FFD {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if id > 0 && id < 0x100000 && crate::timer::is_known_timer_id(id) {
            let dispose = well_known_symbol("dispose");
            if !dispose.is_null() {
                let dispose_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(dispose as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(dispose_f64) {
                    let mname = b"@@__perry_wk_dispose";
                    return crate::object::js_class_method_bind(
                        obj_f64,
                        mname.as_ptr(),
                        mname.len(),
                    );
                }
            }
        }
    }
    // Generic small-handle `Symbol.dispose` support. Subsystems that expose
    // a dispose method through HANDLE_PROPERTY_DISPATCH can bind it here
    // without adding a runtime-specific special case.
    if (bits >> 48) == 0x7FFD {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if id > 0 && id < 0x100000 {
            let dispose = well_known_symbol("dispose");
            if !dispose.is_null() {
                let dispose_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(dispose as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(dispose_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let method = b"@@__perry_wk_dispose";
                        let v = dispatch(id, method.as_ptr(), method.len());
                        if v.to_bits() != TAG_UNDEFINED {
                            return v;
                        }
                    }
                }
            }
        }
    }
    // Generic small-handle `Symbol.asyncDispose` support. This must run before
    // pointer-backed symbol property lookup so small native handles are not
    // interpreted as heap pointers when the dispatcher owns the method.
    if (bits >> 48) == 0x7FFD {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if id > 0 && id < 0x100000 {
            let async_dispose = well_known_symbol("asyncDispose");
            if !async_dispose.is_null() {
                let async_dispose_f64 = f64::from_bits(
                    crate::value::JSValue::pointer(async_dispose as *const u8).bits(),
                );
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(async_dispose_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let method = b"@@__perry_wk_asyncDispose";
                        let v = dispatch(id, method.as_ptr(), method.len());
                        if v.to_bits() != TAG_UNDEFINED {
                            return v;
                        }
                    }
                }
            }
        }
    }
    // Web Fetch and other stdlib handle-backed values are small ids
    // NaN-boxed as POINTER. A computed `handle[Symbol.iterator]` reaches the
    // symbol resolver directly, bypassing the normal string-key handle
    // property dispatcher. Map the well-known symbol back to the dispatcher so
    // `Headers` can expose its `entries` method as the iterator function.
    if (bits >> 48) == 0x7FFD {
        let id = (bits & 0x0000_FFFF_FFFF_FFFF) as i64;
        if id > 0 && id < 0x100000 {
            let iter_wk = well_known_symbol("iterator");
            if !iter_wk.is_null() {
                let iter_f64 =
                    f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
                if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                    if let Some(dispatch) = crate::object::handle_property_dispatch() {
                        let prop = b"@@iterator";
                        let value = dispatch(id, prop.as_ptr(), prop.len());
                        if value.to_bits() != TAG_UNDEFINED {
                            return value;
                        }
                    }
                }
            }
        }
    }
    if let Some(v) = own_symbol_property(obj_f64, sym_f64) {
        return v;
    }
    if let Some(v) = resolve_explicit_object_prototype_symbol(obj_f64, sym_f64) {
        return v;
    }
    // Buffer extends Uint8Array in Node, so Buffer values must expose
    // @@iterator as values(). Perry's direct Buffer.from() paths often
    // materialize through array-clone fast paths, but runtime-produced
    // Buffers can reach generic iterator lookup first.
    let raw_ptr = crate::value::js_nanbox_get_pointer(obj_f64) as usize;
    if raw_ptr >= 0x10000 && crate::buffer::is_registered_buffer(raw_ptr) {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let mname = b"values";
                return crate::object::js_class_method_bind(obj_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    // #36 / #321: the receiver is a closure whose OWN symbol props miss — walk
    // its static prototype chain (`Object.setPrototypeOf(closure, protoObj)`).
    // effect's `TagClass[TagTypeId]` / `isTag(TagClass)` read symbols off
    // `TagProto`. Bounded depth guards against an accidental cycle.
    if (bits >> 48) == 0x7FFD {
        let ptr = crate::value::js_nanbox_get_pointer(obj_f64) as usize;
        if ptr != 0 && crate::closure::is_closure_ptr(ptr) {
            let mut cur = ptr;
            let mut depth = 0usize;
            while depth < 8 {
                let Some(proto_bits) = crate::closure::closure_static_prototype(cur) else {
                    break;
                };
                let proto_f64 = f64::from_bits(proto_bits);
                let proto_ptr = crate::value::js_nanbox_get_pointer(proto_f64) as usize;
                if proto_ptr == 0 || proto_ptr == cur {
                    break;
                }
                if let Some(v) = own_symbol_property(proto_f64, sym_f64) {
                    return v;
                }
                // A class-object proto may carry the symbol through ITS own
                // class_id prototype chain (effect's TagProto spreads
                // EffectPrototype). Walk that before following the closure link.
                let proto_obj = crate::value::JSValue::from_bits(proto_bits)
                    .as_pointer::<crate::object::ObjectHeader>();
                if !proto_obj.is_null() {
                    let cid = crate::object::js_object_get_class_id(proto_obj);
                    if cid != 0 {
                        if let Some(v) = crate::object::resolve_proto_chain_symbol(cid, sym_f64) {
                            return v;
                        }
                    }
                }
                if crate::closure::is_closure_ptr(proto_ptr) {
                    cur = proto_ptr;
                    depth += 1;
                    continue;
                }
                break;
            }
        }
    }
    // #1545: Web ReadableStream handles are normal finite numbers, not
    // heap objects. Expose `rs[Symbol.asyncIterator]` as the same bound method
    // as `rs.values`, matching Node's Web Streams surface while leaving
    // `Symbol.iterator` absent.
    if obj_f64.is_finite() && obj_f64 > 0.0 && obj_f64.fract() == 0.0 {
        if let Some(kind_probe) = crate::object::stream_handle_kind_probe() {
            if kind_probe(obj_f64 as usize) == 1 {
                let async_iterator = well_known_symbol("asyncIterator");
                if !async_iterator.is_null() {
                    let async_iterator_f64 = f64::from_bits(
                        crate::value::JSValue::pointer(async_iterator as *const u8).bits(),
                    );
                    if sym_key_from_f64(sym_f64) == sym_key_from_f64(async_iterator_f64) {
                        let mname = b"values";
                        return crate::object::js_class_method_bind(
                            obj_f64,
                            mname.as_ptr(),
                            mname.len(),
                        );
                    }
                }
            }
        }
    }
    // Buffers inherit TypedArray iteration semantics in Node: the default
    // iterator is `values()`, yielding numeric bytes.
    let raw_addr = if (bits >> 48) >= 0x7FF8 {
        (bits & POINTER_MASK) as usize
    } else {
        bits as usize
    };
    if raw_addr >= 0x1000 && crate::buffer::is_registered_buffer(raw_addr) {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(raw_addr as i64).to_bits());
                let mname = b"values";
                return crate::object::js_class_method_bind(this_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    if raw_addr >= 0x1000 && crate::typedarray::lookup_typed_array_kind(raw_addr).is_some() {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let this_f64 =
                    f64::from_bits(crate::value::js_nanbox_pointer(raw_addr as i64).to_bits());
                let mname = b"values";
                return crate::object::js_class_method_bind(this_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    // #321: arrays expose `Symbol.iterator`. perry has no standalone array
    // iterator object (for-of is special-cased), but `arr[Symbol.iterator]`
    // must resolve to a callable so `Symbol.iterator in arr` is true
    // (effect's `Predicate.isIterable`) and `typeof arr[Symbol.iterator]` is
    // "function". Bind the array's `values` method as that callable. Pre-fix
    // the symbol key fell through to the numeric/string paths and read back a
    // number, so `isIterable([...])` was false and `Effect.all`'s
    // predicate-`dual` `forEach` went data-last (returned a function).
    if crate::array::js_array_is_array(obj_f64).to_bits() == crate::value::TAG_TRUE {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                let mname = b"values";
                return crate::object::js_class_method_bind(obj_f64, mname.as_ptr(), mname.len());
            }
        }
    }
    // #2856: `Map.prototype[Symbol.iterator]` aliases `entries`, and
    // `Set.prototype[Symbol.iterator]` aliases `values`. Bind the matching
    // method so `m[Symbol.iterator]()` returns a real iterator object (and
    // `Symbol.iterator in m` / `typeof m[Symbol.iterator]` are correct).
    if raw_addr >= 0x10000 {
        let iter_wk = well_known_symbol("iterator");
        if !iter_wk.is_null() {
            let iter_f64 =
                f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
            if sym_key_from_f64(sym_f64) == sym_key_from_f64(iter_f64) {
                if crate::map::is_registered_map(raw_addr) {
                    let mname = b"entries";
                    return crate::object::js_class_method_bind(
                        obj_f64,
                        mname.as_ptr(),
                        mname.len(),
                    );
                }
                if crate::set::is_registered_set(raw_addr) {
                    let mname = b"values";
                    return crate::object::js_class_method_bind(
                        obj_f64,
                        mname.as_ptr(),
                        mname.len(),
                    );
                }
            }
        }
    }
    // #1758: a POINTER class-object whose OWN symbol props miss may inherit
    // the symbol through its class_id prototype chain. (The SYMBOL_PROPERTIES
    // lock is released above before recursing into the resolver, which takes
    // it again per prototype object.)
    if (bits >> 48) == 0x7FFD {
        let obj_ptr =
            crate::value::JSValue::from_bits(bits).as_pointer::<crate::object::ObjectHeader>();
        if !obj_ptr.is_null() {
            let cid = crate::object::js_object_get_class_id(obj_ptr);
            if cid != 0 {
                if let Some(v) = crate::object::resolve_proto_chain_symbol(cid, sym_f64) {
                    return v;
                }
                // #1838: a class can define a computed well-known-symbol METHOD
                // (`[Symbol.iterator]() {}`) — class lowering names it
                // `@@iterator` in the vtable (class_members.rs), NOT as a symbol
                // property, so the proto-chain symbol walk above misses it. Map
                // the well-known symbol back to its `@@name`, and if the class
                // (or an ancestor) has that method, return it bound to the
                // instance. This is how effect's `EffectPrimitive` exposes
                // `Symbol.iterator` (→ `SingleShotGen`), so `yield* effectValue`
                // / `Symbol.iterator in effectValue` resolve.
                if let Some(at_name) = well_known_symbol_method_key(sym_f64) {
                    if class_chain_has_method(cid, at_name) {
                        return crate::object::js_class_method_bind(
                            obj_f64,
                            at_name.as_ptr(),
                            at_name.len(),
                        );
                    }
                }
            }
        }
    }
    f64::from_bits(TAG_UNDEFINED)
}

/// #1838: map a well-known symbol value to the synthetic `@@<name>` vtable key
/// that class lowering assigns to a computed `[Symbol.X]() {}` method (see
/// `lower_decl/class_members.rs`). Returns `None` for symbols that don't name a
/// class method (or non-symbol values). `dispose`/`asyncDispose` use distinct
/// `__perry_*__` names and are dispatched via the using-block desugarer, so
/// they're deliberately excluded here.
unsafe fn well_known_symbol_method_key(sym_f64: f64) -> Option<&'static str> {
    let sk = sym_key_from_f64(sym_f64);
    if sk == 0 {
        return None;
    }
    for (short, at_name) in [
        ("iterator", "@@iterator"),
        ("asyncIterator", "@@asyncIterator"),
        ("hasInstance", "@@hasInstance"),
        ("toPrimitive", "@@toPrimitive"),
        ("toStringTag", "@@toStringTag"),
    ] {
        let wk = well_known_symbol(short);
        if !wk.is_null() {
            let wk_f64 = f64::from_bits(crate::value::JSValue::pointer(wk as *const u8).bits());
            if sym_key_from_f64(wk_f64) == sk {
                return Some(at_name);
            }
        }
    }
    None
}

/// #1838: does `class_id` or any ancestor define a vtable method named `name`?
fn class_chain_has_method(class_id: u32, name: &str) -> bool {
    let mut cid = class_id;
    let mut depth = 0usize;
    while depth < 32 && cid != 0 {
        if crate::object::class_has_own_method(cid, name) {
            return true;
        }
        match crate::object::get_parent_class_id(cid) {
            Some(p) if p != 0 && p != cid => {
                cid = p;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}

fn is_object_value(value: f64) -> bool {
    let jv = crate::value::JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return false;
    }
    let raw = crate::value::js_nanbox_get_pointer(value) as usize;
    raw >= 0x10000 && !is_registered_symbol(raw)
}

#[cold]
fn throw_iterator_result_not_object() -> ! {
    let msg = b"Result of the Symbol.iterator method is not an object";
    let msg_str = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let err = crate::error::js_typeerror_new(msg_str);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
}

/// #1831: resolve the iterator for a `yield*` operand.
///
/// `yield* X` must drive `X[Symbol.iterator]()` — for a generator **call** the
/// result already *is* its iterator (perry's generator object is
/// `{next,return,throw}` with no `Symbol.iterator`), but for an arbitrary
/// iterable (effect's `EffectPrimitive`, custom `[Symbol.iterator]` objects)
/// the iterator must first be obtained by invoking the well-known-symbol
/// method. This helper returns that iterator, or `val` unchanged when `val` is
/// already an iterator / not iterable.
///
/// Arrays now route through `array_values_iter` — the runtime has a real
/// `.next`-bearing iterator (`ARRAY_ITERATOR_CLASS_ID`) since #321's
/// `arr.values()` dispatch landed, so `yield* [..]` and any other consumer
/// that drives `js_get_iterator(...).next()` works on a plain array. The
/// for-of and spread fast paths still special-case arrays earlier (in the
/// array-memcpy / index-loop arms) so they don't reach this helper.
#[no_mangle]
pub extern "C" fn js_get_iterator(val_f64: f64) -> f64 {
    if crate::array::js_array_is_array(val_f64).to_bits() == crate::value::TAG_TRUE {
        return crate::array::array_values_iter(val_f64);
    }
    let iter_wk = well_known_symbol("iterator");
    if !iter_wk.is_null() {
        let sym_f64 = f64::from_bits(crate::value::JSValue::pointer(iter_wk as *const u8).bits());
        let iter_fn = unsafe { js_object_get_symbol_property(val_f64, sym_f64) };
        if iter_fn.to_bits() != TAG_UNDEFINED {
            // #321: the `[Symbol.iterator]` method may be INHERITED from a
            // prototype object literal (effect's `EffectPrototype`), in which
            // case codegen baked `this` to the prototype object at definition
            // time (CAPTURES_THIS_FLAG). Per spec `iterable[Symbol.iterator]()`
            // must run with `this === iterable`, so the method reads the real
            // receiver — effect's body is `new SingleShotGen(new YieldWrap(this))`
            // and wraps the wrong value if `this` stays the prototype. Rebind
            // `this` to the original value; a no-op for closures that don't
            // capture `this`.
            let rebound = crate::closure::clone_closure_rebind_this(iter_fn.to_bits(), val_f64);
            let call_target = f64::from_bits(rebound);
            let fn_ptr = crate::value::js_nanbox_get_pointer(call_target)
                as *const crate::closure::ClosureHeader;
            if !fn_ptr.is_null() {
                let iter = crate::closure::js_closure_call0(fn_ptr);
                // Several Perry host-backed collections expose iterator
                // helpers as eager arrays for direct `.entries()` parity. When
                // the same function is reached through `Symbol.iterator`, wrap
                // that array in the runtime array iterator so generic protocol
                // consumers can drive `.next()`.
                if crate::array::js_array_is_array(iter).to_bits() == crate::value::TAG_TRUE {
                    return crate::array::array_values_iter(iter);
                }
                if !is_object_value(iter) {
                    throw_iterator_result_not_object();
                }
                return iter;
            }
        }
    }
    val_f64
}

/// `Object.getOwnPropertySymbols(obj)` — returns an array of symbol keys on
/// the object. Looks up the side table populated by
/// `js_object_set_symbol_property`.
///
/// Returns a raw `*mut ArrayHeader` as i64 (unboxed). Callers should NaN-box
/// with POINTER_TAG before handing the result to user code.
#[no_mangle]
pub unsafe extern "C" fn js_object_get_own_property_symbols(obj_f64: f64) -> i64 {
    // #2818: ToObject(null/undefined) throws TypeError, matching Node. Other
    // primitives box successfully and enumerate no own symbols (empty array).
    let jv = crate::JSValue::from_bits(obj_f64.to_bits());
    if jv.is_null() || jv.is_undefined() {
        crate::object::has_own_helpers::throw_to_object_nullish_type_error();
    }
    let obj_key = obj_key_from_f64(obj_f64);
    if obj_key == 0 {
        return crate::array::js_array_alloc(0) as i64;
    }
    let guard = crate::gc::lock_gc_root_registry(&SYMBOL_PROPERTIES);
    let entries = match guard.as_ref().and_then(|m| m.get(&obj_key)) {
        Some(v) if !v.is_empty() => v.clone(),
        _ => return crate::array::js_array_alloc(0) as i64,
    };
    drop(guard);
    let mut arr = crate::array::js_array_alloc(entries.len() as u32);
    for (sym_ptr_usize, _val_bits) in entries.iter() {
        // Re-NaN-box each symbol pointer with POINTER_TAG so the array
        // contains JSValues that round-trip to user code as Symbols.
        let boxed = f64::from_bits(POINTER_TAG | (*sym_ptr_usize as u64 & POINTER_MASK));
        arr = crate::array::js_array_push_f64(arr, boxed);
    }
    arr as i64
}

/// Return the `typeof` string for a symbol value: "symbol".
/// Codegen can call this in the runtime type-tag dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_typeof() -> *mut StringHeader {
    let s = b"symbol";
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
}

/// Set a method on an object keyed by a symbol. Mirrors
/// `js_object_set_symbol_property` but ALSO binds the closure's reserved
/// `this` slot to `obj_f64` so `[Symbol.toPrimitive](hint) { return this.value }`
/// reads the container when called from `js_to_primitive` at runtime.
///
/// Layout assumption: the last capture slot is the reserved `this` slot
/// (matches `lower_object_literal`'s patching for static-key methods).
/// Only used by HIR for computed-key method props with `captures_this=true`.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_symbol_method(
    obj_f64: f64,
    sym_f64: f64,
    closure_f64: f64,
) -> f64 {
    let c_bits = closure_f64.to_bits();
    let c_tag = c_bits & 0xFFFF_0000_0000_0000;
    if c_tag == POINTER_TAG {
        let c_ptr = (c_bits & POINTER_MASK) as *mut crate::closure::ClosureHeader;
        if !c_ptr.is_null() && (c_ptr as usize) >= 0x1000 {
            // Read the type_tag at offset 12 (layout: func_ptr u64, capture_count u32, type_tag u32).
            let type_tag = std::ptr::read_volatile((c_ptr as *const u8).add(12) as *const u32);
            if type_tag == crate::closure::CLOSURE_MAGIC {
                let raw_count = (*c_ptr).capture_count;
                let real_count = crate::closure::real_capture_count(raw_count);
                if real_count >= 1 {
                    let captures_ptr = (c_ptr as *mut u8)
                        .add(std::mem::size_of::<crate::closure::ClosureHeader>())
                        as *mut f64;
                    *captures_ptr.add((real_count - 1) as usize) = obj_f64;
                }
            }
        }
    }
    js_object_set_symbol_property_infer_name(obj_f64, sym_f64, closure_f64)
}

/// #809: string-key analog of [`js_object_set_symbol_method`]. Sets
/// `obj[key] = closure` by NAME (not the symbol side-table) and ALSO binds
/// the closure's reserved `this` slot to `obj_f64` so a method written
/// AFTER a `...spread` in an object literal still reads the right receiver.
///
/// Used by the ordered-IIFE lowering of object literals that interleave a
/// spread with `this`-binding methods (Effect `HashRing.ts` `Proto`). The
/// non-spread fast path patches `this` post-build in codegen; this helper
/// is the runtime equivalent for the ordered path where the closure flows
/// in as a call argument.
///
/// Layout assumption (identical to `js_object_set_symbol_method`): the
/// LAST capture slot is the reserved `this` slot.
#[no_mangle]
pub unsafe extern "C" fn js_object_set_method_by_name(
    obj_f64: f64,
    key_f64: f64,
    closure_f64: f64,
) -> f64 {
    // 1) Patch the closure's reserved (last) `this` capture slot with obj.
    let c_bits = closure_f64.to_bits();
    let c_tag = c_bits & 0xFFFF_0000_0000_0000;
    if c_tag == POINTER_TAG {
        let c_ptr = (c_bits & POINTER_MASK) as *mut crate::closure::ClosureHeader;
        if !c_ptr.is_null() && (c_ptr as usize) >= 0x1000 {
            let type_tag = std::ptr::read_volatile((c_ptr as *const u8).add(12) as *const u32);
            if type_tag == crate::closure::CLOSURE_MAGIC {
                let raw_count = (*c_ptr).capture_count;
                let real_count = crate::closure::real_capture_count(raw_count);
                if real_count >= 1 {
                    let captures_ptr = (c_ptr as *mut u8)
                        .add(std::mem::size_of::<crate::closure::ClosureHeader>())
                        as *mut f64;
                    *captures_ptr.add((real_count - 1) as usize) = obj_f64;
                }
            }
        }
    }

    // 2) Set the field by name. `js_object_set_field_by_name` strips the
    //    NaN-box tag off `obj` itself, so passing the raw bits is fine; the
    //    key must be a real `StringHeader*` (tag stripped).
    let key_bits = key_f64.to_bits();
    let key_ptr = (key_bits & POINTER_MASK) as *const StringHeader;
    let obj_ptr = obj_f64.to_bits() as *mut crate::object::ObjectHeader;
    if !key_ptr.is_null() && (key_ptr as usize) >= 0x1000 {
        crate::object::js_object_set_field_by_name(obj_ptr, key_ptr, closure_f64);
    }
    obj_f64
}

/// `ToPrimitive(value, hint)` — if `value` is an object with a
/// `[Symbol.toPrimitive]` method registered in the symbol side-table, call
/// it with the appropriate hint string ("number" / "string" / "default")
/// and return the primitive result. Otherwise returns `value` unchanged.
///
/// `hint`: 0 = default, 1 = number, 2 = string.
///
/// Used by `js_number_coerce` (unary `+`, binary `+` numeric coercion),
/// `js_jsvalue_to_string` (template literals, String(x)), and the
/// lower_string_coerce_concat path.
#[no_mangle]
pub unsafe extern "C" fn js_to_primitive(value: f64, hint: i32) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let value_handle = scope.root_nanbox_f64(value);
    let value = value_handle.get_nanbox_f64();
    let bits = value.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    if tag != POINTER_TAG {
        return value;
    }
    let obj_ptr = (bits & POINTER_MASK) as usize;
    if obj_ptr < 0x1000 {
        return value;
    }
    // Skip symbols / buffers / arrays — they have their own coercion rules.
    if is_registered_symbol(obj_ptr) {
        return value;
    }
    // Look up obj[Symbol.toPrimitive].
    let wk_ptr = well_known_symbol("toPrimitive");
    let sym_f64 = f64::from_bits(POINTER_TAG | (wk_ptr as u64 & POINTER_MASK));
    let current_value = value_handle.get_nanbox_f64();
    let method = js_object_get_symbol_property(current_value, sym_f64);
    if method.to_bits() == TAG_UNDEFINED {
        return current_value;
    }
    // Method must be a closure pointer.
    let method_bits = method.to_bits();
    let method_tag = method_bits & 0xFFFF_0000_0000_0000;
    if method_tag != POINTER_TAG {
        return value_handle.get_nanbox_f64();
    }
    let method_handle = scope.root_nanbox_f64(method);
    let closure_ptr = (method_bits & POINTER_MASK) as *const crate::closure::ClosureHeader;
    if closure_ptr.is_null() || (closure_ptr as usize) < 0x1000 {
        return value_handle.get_nanbox_f64();
    }
    // Validate CLOSURE_MAGIC before calling.
    let type_tag = std::ptr::read_volatile((closure_ptr as *const u8).add(12) as *const u32);
    if type_tag != crate::closure::CLOSURE_MAGIC {
        return value_handle.get_nanbox_f64();
    }
    let hint_str: &[u8] = match hint {
        1 => b"number",
        2 => b"string",
        _ => b"default",
    };
    let hint_ptr = js_string_from_bytes(hint_str.as_ptr(), hint_str.len() as u32);
    let hint_handle = scope.root_string_ptr(hint_ptr);
    let hint_f64 = f64::from_bits(
        STRING_TAG | (hint_handle.get_raw_const_ptr::<StringHeader>() as u64 & POINTER_MASK),
    );
    let method_bits = method_handle.get_nanbox_f64().to_bits();
    let closure_ptr = (method_bits & POINTER_MASK) as *const crate::closure::ClosureHeader;

    // Spec says the return value must be a primitive; if it's still an
    // object pointer, that's a TypeError in JS, but we just return it
    // as-is and let the caller fall back.
    crate::closure::js_closure_call1(closure_ptr, hint_f64)
}

/// Compare two Symbol JSValues for equality. Two symbols are equal iff they
/// point to the same SymbolHeader (including Symbol.for dedup).
#[no_mangle]
pub unsafe extern "C" fn js_symbol_equals(a: f64, b: f64) -> i32 {
    let abits = a.to_bits();
    let bbits = b.to_bits();
    if abits == bbits {
        return 1;
    }
    let atag = abits & 0xFFFF_0000_0000_0000;
    let btag = bbits & 0xFFFF_0000_0000_0000;
    if atag != POINTER_TAG || btag != POINTER_TAG {
        return 0;
    }
    let aptr = (abits & POINTER_MASK) as *const SymbolHeader;
    let bptr = (bbits & POINTER_MASK) as *const SymbolHeader;
    if aptr.is_null() || bptr.is_null() {
        return 0;
    }
    if (*aptr).magic != SYMBOL_MAGIC || (*bptr).magic != SYMBOL_MAGIC {
        return 0;
    }
    if (*aptr).id == (*bptr).id {
        1
    } else {
        0
    }
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
