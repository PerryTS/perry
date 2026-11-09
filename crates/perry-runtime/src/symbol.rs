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
//! Symbols are heap objects allocated in the arena with
//! `GC_TYPE_SYMBOL`; their descriptions are traced child edges. They are
//! NaN-boxed with `POINTER_TAG`, which means they
//! round-trip through the runtime as regular pointer JSValues.
//!
//! Dedicated Symbol support requires a small codegen hook (see report):
//! intercepting `Symbol(desc)` / `Symbol.for(key)` / `Symbol.keyFor(sym)` /
//! `Object.getOwnPropertySymbols(obj)` calls and routing them to the
//! functions in this module.

mod accessors;
mod constructors;
mod get;
#[cfg(test)]
mod inherited_accessor_tests;
mod iterator;
mod properties;
mod space;
#[cfg(test)]
mod test_helpers;

pub(crate) use accessors::set_symbol_accessor_property;
#[doc(hidden)]
pub use accessors::symbol_accessor_held_for_test;

// Symbol constructor + value FFI (no_mangle entry points re-exported so existing
// `crate::symbol::js_symbol_*` call paths keep resolving).
pub(crate) use constructors::symbol_for_text;
pub use constructors::{
    js_symbol_description, js_symbol_equals, js_symbol_for, js_symbol_key_for, js_symbol_new,
    js_symbol_new_empty, js_symbol_to_string, js_symbol_typeof,
};

// Symbol-keyed own property operations.
pub(crate) use properties::{
    class_static_symbol_keys_for_class, clone_symbol_entries_for_obj_ptr,
    define_symbol_data_property, get_symbol_property_attrs, inspect_custom_symbol_ptr,
    js_object_define_symbol_accessor, js_object_delete_symbol_property,
    js_object_has_own_symbol_property, reflect_symbol_getter_closure_bits,
    set_symbol_property_attrs, set_symbol_property_with_strictness,
    symbol_accessor_descriptor_bits, symbol_property_is_enumerable,
    symbol_property_is_non_writable, symbol_property_root_bits,
};
pub use properties::{
    class_static_symbol_lookup, class_static_symbol_lookup_in_chain,
    js_class_register_static_symbol, js_object_has_own_symbol,
    js_object_literal_infer_computed_function_name, js_object_set_method_by_name,
    js_object_set_symbol_method, js_object_set_symbol_property,
};

// Symbol-keyed property reads.
pub(crate) use get::has_declared_prototype_symbol_property;
pub use get::js_object_get_symbol_property;
pub(crate) use get::{
    has_own_symbol_property, inherited_symbol_property,
    js_object_get_symbol_property_with_receiver, own_symbol_property, own_symbol_slot,
    OwnSymbolSlot,
};

// Iterator protocol, getOwnPropertySymbols, ToPrimitive.
pub(crate) use iterator::{
    class_ref_resolves_iterator, get_iterator_from_method, iterator_from_method,
};
pub use iterator::{
    js_get_iterator, js_iterator_result_validate, js_object_get_own_property_symbols,
    js_to_primitive,
};

#[cfg(test)]
pub(crate) use test_helpers::{
    test_class_static_symbol_root_bits, test_class_static_symbol_roots_for_class,
    test_seed_class_static_symbol_root, test_seed_symbol_property_root,
    test_symbol_property_root_bits,
};

use crate::string::StringHeader;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

// NaN-boxing tags (must match value.rs)
const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// Magic number identifying a SymbolHeader payload.
/// Placed at offset 0 so `js_is_symbol` can cheaply detect symbols.
pub const SYMBOL_MAGIC: u32 = crate::codegen_abi::SYMBOL_HEADER_MAGIC; // "SYMB"

/// The shared symbol payload; descriptions name the runtime string header.
pub type SymbolHeader = crate::codegen_abi::SymbolHeader<StringHeader>;

/// A persistent symbol's cell in the process immortal symbol space
/// ([`space`]): the same p-8 header contract as GC cells. The prefix is not
/// registered with the collector: identity and lifetime are process-wide,
/// and the payload contains no GC pointers.
#[repr(C)]
struct PersistentSymbol {
    header: crate::gc::GcHeader,
    symbol: SymbolHeader,
    description_text: Option<std::sync::Arc<str>>,
}

fn leak_symbol(mut symbol: SymbolHeader) -> *mut SymbolHeader {
    symbol.registered |= perry_abi::SYMBOL_NATIVE_TEXT;
    #[allow(unused_mut)]
    let mut cell = PersistentSymbol {
        header: crate::gc::GcHeader {
            obj_type: crate::gc::GC_TYPE_SYMBOL,
            // This process-lifetime leaf is never censused or swept. It is
            // permanently black; precise roots may inspect its real header.
            gc_flags: crate::gc::GC_FLAG_TENURED | crate::gc::GC_FLAG_MARKED,
            _reserved: 0,
            // The GC prefix describes only the Symbol payload. The trailing
            // native Arc is process-lifetime storage, not a heap child slot.
            size: (crate::gc::GC_HEADER_SIZE + std::mem::size_of::<SymbolHeader>()) as u32,
        },
        symbol,
        description_text: None,
    };
    #[cfg(test)]
    if crate::buffer::bytes::b4_sabotage("symbol_header") {
        cell.header.obj_type = crate::gc::GC_TYPE_STRING;
    }
    let raw = space::alloc_cell();
    // SAFETY: a fresh, zeroed, exclusively owned cell of the space.
    unsafe {
        // GC_STORE_AUDIT(INIT): fresh zeroed cell of the immortal symbol space, written once before publication; its description slot is installed afterward through the barriered setter.
        raw.write(cell);
        // The space is outside the moving arena. Use the shared setter so
        // pin custody remains explicit even for these immortal leaf cells.
        crate::gc::pin_object(std::ptr::addr_of_mut!((*raw).header));
        std::ptr::addr_of_mut!((*raw).symbol)
    }
}

const _: () = assert!(std::mem::offset_of!(PersistentSymbol, symbol) == crate::gc::GC_HEADER_SIZE);
const _: () = assert!(std::mem::offset_of!(SymbolHeader, registered) == 4);

// Global registry for Symbol.for(key) — maps key → symbol pointer (as usize).
// The symbol pointers stored here are leaked (never freed) so that
// `Symbol.for("x") === Symbol.for("x")` always returns the same pointer.
static SYMBOL_REGISTRY: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

/// Read native, process-lifetime text from a proven Symbol payload. Fresh
/// symbols have no native tail and answer None before it is addressed.
pub(crate) fn registered_symbol_description(sym_ptr: usize) -> Option<std::sync::Arc<str>> {
    unsafe {
        if (*(sym_ptr as *const SymbolHeader)).registered & perry_abi::SYMBOL_NATIVE_TEXT == 0 {
            return None;
        }
        let cell = (sym_ptr - crate::gc::GC_HEADER_SIZE) as *const PersistentSymbol;
        (*cell).description_text.clone()
    }
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
    // Fresh symbols own a real traced description edge. Persistent symbols
    // retain process-lifetime bytes and materialize in the caller's arena.
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

/// Initialize an unpublished persistent symbol's native text, before its
/// owning named registry exposes it to another thread.
pub(crate) fn record_registered_symbol_description(sym_ptr: usize, description: &str) {
    unsafe {
        let cell = (sym_ptr - crate::gc::GC_HEADER_SIZE) as *mut PersistentSymbol;
        debug_assert!((*cell).symbol.registered & perry_abi::SYMBOL_NATIVE_TEXT != 0);
        (*cell).description_text = Some(std::sync::Arc::from(description));
    }
}
#[cfg(test)]
pub(crate) fn test_clear_persistent_description(sym_ptr: usize) {
    unsafe {
        let cell = (sym_ptr - crate::gc::GC_HEADER_SIZE) as *mut PersistentSymbol;
        (*cell).description_text = None;
    }
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
/// Returns the pointer to the cached, process-lifetime `SymbolHeader`.
pub fn well_known_symbol(short_name: &str) -> *mut SymbolHeader {
    // #10510: lock-free hit for the names the runtime itself resolves on hot
    // paths — every implicit `ToPrimitive` and every symbol-keyed read that
    // misses reaches several of these, and the slow path below takes a mutex
    // and hashes a `String` key each time. Published only after the slow path
    // has fully initialized the symbol, and never cleared (the symbols are
    // Box-leaked and `WELL_KNOWN_SYMBOLS` is never reset).
    let fast = well_known_fast_slot(short_name);
    if let Some(slot) = fast {
        let ptr = slot.load(std::sync::atomic::Ordering::Acquire);
        if ptr != 0 {
            return ptr as *mut SymbolHeader;
        }
    }
    let sym_ptr = well_known_symbol_slow(short_name);
    if let Some(slot) = fast {
        slot.store(sym_ptr as usize, std::sync::atomic::Ordering::Release);
    }
    sym_ptr
}

fn well_known_fast_slot(short_name: &str) -> Option<&'static std::sync::atomic::AtomicUsize> {
    static SLOTS: [std::sync::atomic::AtomicUsize; 8] =
        [const { std::sync::atomic::AtomicUsize::new(0) }; 8];
    let index = match short_name {
        "iterator" => 0,
        "asyncIterator" => 1,
        "toPrimitive" => 2,
        "toStringTag" => 3,
        "hasInstance" => 4,
        "dispose" => 5,
        "asyncDispose" => 6,
        "species" => 7,
        _ => return None,
    };
    Some(&SLOTS[index])
}

/// A fresh, process-lifetime symbol (not in the `Symbol.for` registry) with
/// `description`: a symbol of node's own library that a runtime layout lists
/// as a key (`runtime_layouts::runtime_symbol_of_atom`), the same symbol in
/// every agent.
pub(crate) fn persistent_symbol(description: &str) -> *mut SymbolHeader {
    let sym_ptr = leak_symbol(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: 0,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    record_registered_symbol_description(sym_ptr as usize, description);
    sym_ptr
}

fn well_known_symbol_slow(short_name: &str) -> *mut SymbolHeader {
    let mut guard = WELL_KNOWN_SYMBOLS.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let cache = guard.as_mut().unwrap();
    if let Some(&ptr_usize) = cache.get(short_name) {
        return ptr_usize as *mut SymbolHeader;
    }
    // First use: allocate a persistent (leaked) SymbolHeader. Description is
    // null-on-the-header — the actual text lives in the symbol's native text,
    // and readers materialize a StringHeader in their own arena on demand. We
    // can't store a real StringHeader pointer here because this allocation may
    // be made on a worker thread whose arena will later be torn down, while
    // the SymbolHeader itself is Box-leaked and outlives that arena.
    let sym_ptr = leak_symbol(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: perry_abi::SYMBOL_WELL_KNOWN,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::SymbolGlobal,
        );
    }
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

/// Is `ptr` a well-known symbol (Symbol.toPrimitive etc.)? Its own payload
/// says so (`SYMBOL_WELL_KNOWN`); `js_symbol_key_for` keeps the spec's
/// `undefined` for them.
pub fn is_well_known_symbol(ptr: usize) -> bool {
    is_registered_symbol(ptr)
        && unsafe { (*(ptr as *const SymbolHeader)).registered & perry_abi::SYMBOL_WELL_KNOWN != 0 }
}

/// True when the bytes at `ptr` *could* be a [`SymbolHeader`], i.e. when a
/// classifier that has already ruled a symbol out some other way must still ask
/// the authoritative [`is_registered_symbol`].
///
/// Every symbol carries GC_TYPE_SYMBOL and SYMBOL_MAGIC. The magic screen
/// cheaply rejects ordinary receivers before the authoritative ownership probe.
/// `false` is exact; `true` alone does not establish allocation ownership.
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

#[cfg(test)]
thread_local! {
    // Historical test meter: counts entry into the authoritative brand probe.
    static TEST_SYMBOL_REGISTRY_PROBES: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn test_symbol_registry_probe_count() -> u64 {
    TEST_SYMBOL_REGISTRY_PROBES.with(|c| c.get())
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
    // realm-global. Description text lives in the symbol's native text
    // (readers materialize a fresh StringHeader on demand), matching the
    // well-known-symbol contract.
    let sym_ptr = leak_symbol(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: 0,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    let sym_ptr = sym_ptr as usize;
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::SymbolGlobal,
        );
    }
    record_registered_symbol_description(sym_ptr, "IntlLegacyConstructedSymbol");
    *guard = Some(sym_ptr);
    f64::from_bits(POINTER_TAG | (sym_ptr as u64 & POINTER_MASK))
}

/// Is the raw word `ptr` a symbol? A symbol is known by its GC header
/// (DECISIONS 243): every symbol is a cell of type `GC_TYPE_SYMBOL` whose
/// payload starts with `SYMBOL_MAGIC`. No registry is consulted.
///
/// `ptr` may be arbitrary bits (a number, an interior address), so the cell
/// is proven before any read, by where it lives:
/// - a tracked heap range: the allocator's object-start proof;
/// - the process immortal symbol space (every well-known, `Symbol.for`, Intl
///   and runtime symbol): its range and cell stride ([`space::holds`]);
/// - the realm image (codegen's symbol cells): its bounds.
///
/// Any other word is no symbol, and nothing is read. Only then the payload
/// magic and the header type. A NaN-boxed value takes [`js_is_symbol`], which
/// reads the honest pointer's header directly.
#[inline(never)]
pub fn is_registered_symbol(ptr: usize) -> bool {
    #[cfg(test)]
    TEST_SYMBOL_REGISTRY_PROBES.with(|c| c.set(c.get().wrapping_add(1)));
    if let Some(matches) =
        unsafe { crate::value::addr_class::tracked_gc_type_matches(ptr, crate::gc::GC_TYPE_SYMBOL) }
    {
        return matches && unsafe { (*(ptr as *const SymbolHeader)).magic == SYMBOL_MAGIC };
    }
    let cell = space::holds(ptr) || image_symbol_extent(ptr);
    // SAFETY: a cell start of the symbol space, or an aligned image address
    // whose header and payload lie inside the image.
    cell && unsafe { symbol_cell_at(ptr) }
}

/// Could the raw word `ptr` be a symbol cell of the realm image? Its header
/// and a whole symbol payload lie inside the image section, aligned: bounds
/// only, no read.
#[inline(always)]
fn image_symbol_extent(ptr: usize) -> bool {
    crate::value::addr_class::realm::is_image_object(ptr)
        && ptr.is_multiple_of(std::mem::align_of::<crate::gc::GcHeader>())
        && ptr
            .checked_add(std::mem::size_of::<SymbolHeader>())
            .is_some_and(|end| end <= crate::value::addr_class::realm::bounds().1)
}

/// The header test itself: the payload magic, then the header type.
///
/// # Safety
/// `ptr` is an aligned address of readable memory with a readable word
/// before it (an honest pointer payload).
#[inline(always)]
unsafe fn symbol_cell_at(ptr: usize) -> bool {
    may_be_symbol_header(ptr as *const u8)
        && (*((ptr - crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader)).obj_type
            == crate::gc::GC_TYPE_SYMBOL
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
        !sym.is_null()
            && (*sym).magic == SYMBOL_MAGIC
            && (*sym).registered & perry_abi::SYMBOL_GLOBAL_REGISTRY != 0
    }
}

/// [`is_global_registered_symbol`] for a value: the payload-word screen of
/// [`js_is_symbol`] answers every object, array and closure without an address
/// proof, which is what WeakMap/WeakSet/WeakRef key validation sees.
#[inline]
pub(crate) fn value_is_global_registered_symbol(value: f64) -> bool {
    // SAFETY: js_is_symbol proves the cell before the payload is read.
    unsafe {
        js_is_symbol(value) != 0
            && (*(((value.to_bits() & POINTER_MASK) as usize) as *const SymbolHeader)).registered
                & perry_abi::SYMBOL_GLOBAL_REGISTRY
                != 0
    }
}

/// Test probe (#11471): classify only an address live in this thread's heap
/// or an existing immortal named symbol owner. No membership set is kept.
#[doc(hidden)]
pub fn symbol_pointer_registered_for_test(ptr: usize) -> bool {
    is_registered_symbol(ptr)
}
#[doc(hidden)]
pub fn registered_symbol_id_for_test(ptr: usize) -> Option<u64> {
    is_registered_symbol(ptr).then(|| unsafe { (*(ptr as *const SymbolHeader)).id })
}

/// Test probe (#11471): does `owner` have a `SYMBOL_PROPERTIES` record, and a
/// symbol descriptor entry for `sym` in its own shape?
#[doc(hidden)]
pub fn symbol_property_tables_hold_for_test(owner: usize, sym: usize) -> (bool, bool) {
    let props = false; // The owner-keyed data table has been deleted.
    let attrs = get_symbol_property_attrs(owner, sym).is_some();
    (props, attrs)
}

/// Test probe (#11471): the owner key class `class_id`'s static symbol
/// members are stored under in the calling agent — its function object.
#[doc(hidden)]
pub fn class_static_symbol_owner_for_test(class_id: u32) -> usize {
    crate::object::class_value::class_value_ptr(class_id) as usize
}

/// Test probe (#11696): is `sym` an own key of the ordinary object `owner`'s
/// shape (#11682 stores those on the object, not in `SYMBOL_PROPERTIES`)?
/// `Some(is_accessor)` when it is, `None` when `owner` is not an ordinary
/// object or has no such key.
#[doc(hidden)]
pub fn symbol_on_object_for_test(owner: usize, sym: usize) -> Option<bool> {
    // SAFETY: `owner` validates the address as a tracked GC object first.
    unsafe {
        crate::object::shaped_symbols::owner(owner)?;
        crate::object::shaped_symbols::entry(owner, sym)
            .map(|entry| entry & crate::object::key_attrs::ENTRY_ACCESSOR != 0)
    }
}

// The counter provides uniqueness only; symbol publication has its own locks.
// Dynamic births count up from 1; image symbols own the top ordinals.
static NEXT_SYMBOL_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    // Computed once at birth; every key path still loads the same id.
    crate::codegen_abi::symbol_identity(NEXT_SYMBOL_ID.fetch_add(1, Ordering::Relaxed))
}

/// Recover creation order for the defensive own-symbol enumeration fallback.
/// The mix is invertible, so it loses no identity; image symbols rank first.
fn creation_order(id: u64) -> u64 {
    crate::codegen_abi::symbol_birth_rank(id)
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
    // Initialization publishes only native fields, without user code/polls.
    // The descriptor owns the description; it follows the symbol on moves.
    let _no_move = crate::gc::GcSuppressScope::new();
    let raw = crate::arena::arena_alloc_gc(
        std::mem::size_of::<SymbolHeader>(),
        std::mem::align_of::<SymbolHeader>(),
        crate::gc::GC_TYPE_SYMBOL,
    );
    let ptr = raw as *mut SymbolHeader;
    // GC_STORE_AUDIT(BARRIERED): alloc_symbol holds GcSuppressScope during fresh initialization; description pointer is then installed through runtime_store_gc_heap_word_slot before scope exits (symbol.rs606-626).
    std::ptr::write(
        ptr,
        SymbolHeader {
            magic: SYMBOL_MAGIC,
            registered: u32::from(registered),
            description,
            id: next_id(),
        },
    );
    crate::gc::runtime_store_gc_heap_word_slot(
        ptr as usize,
        std::ptr::addr_of_mut!((*ptr).description) as usize,
        description as u64,
    );
    ptr
}

/// Check whether a NaN-boxed JSValue is a Symbol: the pointer tag, then the
/// cell's own header (DECISIONS 243). A `POINTER_TAG` word above the handle
/// band names a GC-headed cell (`addr_class::pointer_value_header`), in the
/// arena or in immortal space alike, so no allocation proof and no registry
/// is consulted. The payload magic is read first: every object, array and
/// closure receiver answers "no" from its own first word.
#[no_mangle]
pub unsafe extern "C" fn js_is_symbol(value: f64) -> i32 {
    let bits = value.to_bits();
    const BIAS: u64 = POINTER_TAG | crate::value::addr_class::HANDLE_BAND_MAX as u64;
    const SPAN: u64 = (1u64 << 48) - crate::value::addr_class::HANDLE_BAND_MAX as u64;
    if bits.wrapping_sub(BIAS) >= SPAN {
        return 0;
    }
    i32::from(symbol_cell_at((bits & POINTER_MASK) as usize))
}

/// Extract the raw object pointer from a NaN-boxed JSValue. Returns 0 if the
/// value isn't a pointer-tagged object (and 0 is also a valid "no entries"
/// sentinel for the side table).
pub(crate) unsafe fn obj_key_from_f64(obj_f64: f64) -> usize {
    let bits = obj_f64.to_bits();
    let tag = bits & 0xFFFF_0000_0000_0000;
    // A native id keys its own symbol entries by id; ids never collide with
    // heap addresses (they sit below the id-band ceiling).
    if !crate::value::is_reference_bits(tag) {
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

pub(crate) fn store_object_symbol_property_root(
    obj_key: usize,
    sym_key: usize,
    value_bits: u64,
) -> bool {
    note_symbol_key_installed(sym_key);
    let _holder_edit = crate::object::descriptor_state::HolderEdit::new(obj_key);
    if unsafe { crate::object::shaped_symbols::owner(obj_key).is_some() } {
        let existed = unsafe { crate::object::shaped_symbols::entry(obj_key, sym_key) };
        let entry = existed.unwrap_or(0) & !crate::object::key_attrs::ENTRY_ACCESSOR_MASK;
        unsafe {
            crate::object::shaped_symbols::define(obj_key, sym_key, value_bits, entry);
        }
        return existed.is_none();
    }
    debug_assert!(false, "symbol data owner must have a property holder");
    false
}

/// Idle until a class declares a static Symbol-keyed member.
///
/// `js_instanceof` consults a class's static symbols for a `Symbol.hasInstance`
/// override on EVERY evaluation, which meant a process-global `Mutex` plus a
/// SipHash probe of an empty map for every `x instanceof C` in a program that
/// never mentions a Symbol (#7769).
pub(crate) static CLASS_STATIC_SYMBOLS_LATCH: crate::registry_latch::RegistryLatch =
    crate::registry_latch::RegistryLatch::new();

/// A class's static Symbol-keyed data property (`static [sym] = v`,
/// `C[sym] = v`): an own symbol property of the class's function object, in
/// the same per-object store every object uses (the object is pinned, so its
/// address is a stable owner key).
pub(crate) fn store_class_static_symbol_root(class_id: u32, sym_key: usize, value_bits: u64) {
    CLASS_STATIC_SYMBOLS_LATCH.arm();
    let owner = crate::object::class_value::class_value_ptr(class_id) as usize;
    store_object_symbol_property_root(owner, sym_key, value_bits);
}

/// Is the raw word `addr` a cell of the process immortal symbol space
/// ([`space::holds`]: range and cell stride, no read)? For the snapshot
/// census key-name rule, which names a persistent symbol by what it is
/// (kind and description), never by its cell address: cells are bump
/// allocated in creation order, so an address is a creation ordinal.
#[cfg(all(perry_gc_instruments, target_os = "linux", target_arch = "x86_64"))]
#[inline(always)]
pub(crate) fn in_symbol_space(addr: usize) -> bool {
    space::holds(addr)
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

/// `PERRY_GC_CENSUS`: the `Symbol.for` registry.
pub(crate) fn symbol_registry_census() -> crate::gc::census::SideTableRow {
    use crate::gc::census::map_bytes;
    if let Ok(g) = SYMBOL_REGISTRY.lock() {
        if let Some(m) = g.as_ref() {
            let inner: usize = m.keys().map(|k| k.capacity()).sum();
            return ("symbol.registry", m.len(), map_bytes(m) + inner);
        }
    }
    ("symbol.registry", 0, 0)
}

#[cfg(test)]
mod symbol_identity_is_header {
    use super::*;
    use crate::gc::{GcHeader, GC_FLAG_MARKED, GC_FLAG_TENURED, GC_HEADER_SIZE, GC_TYPE_SYMBOL};

    fn header(obj_type: u8, gc_flags: u8) -> GcHeader {
        GcHeader {
            obj_type,
            gc_flags,
            _reserved: 0,
            size: (GC_HEADER_SIZE + std::mem::size_of::<SymbolHeader>()) as u32,
        }
    }

    fn payload(magic: u32) -> SymbolHeader {
        SymbolHeader {
            magic,
            registered: 0,
            description: std::ptr::null_mut(),
            id: 0x5A11,
        }
    }

    /// A hand-written cell of the process symbol space (a forgery when the
    /// type or magic is wrong): the space proves the address, the header
    /// decides.
    fn space_cell(obj_type: u8, magic: u32) -> usize {
        let raw = space::alloc_cell();
        unsafe {
            // GC_STORE_AUDIT(INIT): test-only forged cell in a fresh symbol-space slot; description is null and nothing is published before the write.
            raw.write(PersistentSymbol {
                header: header(obj_type, GC_FLAG_TENURED | GC_FLAG_MARKED),
                symbol: payload(magic),
                description_text: None,
            });
            std::ptr::addr_of!((*raw).symbol) as usize
        }
    }

    #[repr(C)]
    struct ImageSymbol {
        header: GcHeader,
        symbol: SymbolHeader,
    }

    const fn image_symbol() -> ImageSymbol {
        ImageSymbol {
            header: GcHeader {
                obj_type: GC_TYPE_SYMBOL,
                gc_flags: crate::value::addr_class::realm::IMAGE_CELL_FLAGS,
                _reserved: 0,
                size: (GC_HEADER_SIZE + std::mem::size_of::<SymbolHeader>()) as u32,
            },
            symbol: SymbolHeader {
                magic: SYMBOL_MAGIC,
                registered: 0,
                description: std::ptr::null_mut(),
                id: 0x5A12,
            },
        }
    }

    /// A codegen-style symbol cell in the realm image section.
    #[used]
    #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_realm")]
    #[cfg_attr(target_os = "windows", link_section = ".perryrealm$m")]
    #[cfg_attr(
        not(any(target_vendor = "apple", target_os = "windows")),
        link_section = "perry_realm"
    )]
    static mut IMAGE_SYMBOL: ImageSymbol = image_symbol();
    /// The same bytes in ordinary data: only the image bounds tell them apart.
    #[used]
    static mut COUNTERFEIT_SYMBOL: ImageSymbol = image_symbol();

    fn boxed(ptr: usize) -> f64 {
        f64::from_bits(POINTER_TAG | ptr as u64)
    }

    /// The value form (honest header read) and the raw-word form (address
    /// proof first) agree on every real cell.
    fn accepted(ptr: usize) -> bool {
        let by_value = unsafe { js_is_symbol(boxed(ptr)) } != 0;
        assert_eq!(
            by_value,
            is_registered_symbol(ptr),
            "value and word forms agree"
        );
        by_value
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn symbol_identity_is_header() {
        // SYMBOL_MAGIC in a cell whose header is not a symbol's: rejected.
        assert!(!accepted(space_cell(
            crate::gc::GC_TYPE_OBJECT,
            SYMBOL_MAGIC
        )));
        assert!(!accepted(space_cell(
            crate::gc::GC_TYPE_STRING,
            SYMBOL_MAGIC
        )));
        // A symbol header over a payload without the magic: rejected.
        assert!(!accepted(space_cell(GC_TYPE_SYMBOL, !SYMBOL_MAGIC)));
        // A symbol cell no registry owns is a symbol: the header is the proof.
        // (A registry-membership test rejects this one.)
        let bare = space_cell(GC_TYPE_SYMBOL, SYMBOL_MAGIC);
        assert!(accepted(bare));
        // Every persistent kind, and an arena symbol.
        let wk = well_known_symbol("iterator") as usize;
        let rt = persistent_symbol("kNxpaRuntime") as usize;
        let intl = (intl_legacy_constructed_symbol().to_bits() & POINTER_MASK) as usize;
        let key = crate::string::js_string_from_bytes(b"nxpa.for".as_ptr(), 8);
        let for_value = unsafe { js_symbol_for(f64::from_bits(STRING_TAG | key as u64)) };
        let reg = (for_value.to_bits() & POINTER_MASK) as usize;
        let local = unsafe { alloc_symbol(std::ptr::null_mut(), false) } as usize;
        for (name, ptr) in [
            ("well-known", wk),
            ("runtime", rt),
            ("intl", intl),
            ("for", reg),
            ("local", local),
        ] {
            assert!(accepted(ptr), "{name} symbol is known by its header");
        }
        // Every persistent kind lives in the one process symbol space.
        for (name, ptr) in [
            ("well-known", wk),
            ("runtime", rt),
            ("intl", intl),
            ("for", reg),
        ] {
            assert!(
                space::holds(ptr),
                "{name} symbol is a cell of the symbol space"
            );
        }
        assert!(!space::holds(local));
        // Kinds come from the payload bits.
        assert!(is_well_known_symbol(wk));
        assert!(
            !is_well_known_symbol(rt) && !is_well_known_symbol(reg) && !is_well_known_symbol(local)
        );
        assert!(is_global_registered_symbol(reg));
        assert!(
            !is_global_registered_symbol(wk)
                && !is_global_registered_symbol(rt)
                && !is_global_registered_symbol(local)
        );
        // WeakRef / FinalizationRegistry targets: a registered symbol is refused,
        // the others are held weakly.
        assert!(!crate::weakref::is_valid_weak_target(for_value));
        assert!(crate::weakref::is_valid_weak_target(boxed(wk)));
        assert!(crate::weakref::is_valid_weak_target(boxed(local)));

        // Raw words: the address is proven before anything is read.
        // Interior and misaligned words of a real space cell: rejected.
        for raw in [
            bare - GC_HEADER_SIZE,
            bare + 8,
            bare + 16,
            bare + 1,
            bare + 4,
            bare + space::CELL - 8,
        ] {
            assert!(!is_registered_symbol(raw), "space interior {raw:#x}");
        }
        // The last cell of the range, past the bump (never handed out): rejected.
        let last = space::base() + (space::CELLS - 1) * space::CELL + GC_HEADER_SIZE;
        assert!(!is_registered_symbol(last));
        // An image symbol cell: accepted by the image bounds; the same bytes
        // in ordinary data are not a cell the raw-word form can prove.
        let image = unsafe { std::ptr::addr_of!(IMAGE_SYMBOL.symbol) } as usize;
        assert!(crate::value::addr_class::realm::is_image_object(image));
        assert!(accepted(image));
        let counterfeit = unsafe { std::ptr::addr_of!(COUNTERFEIT_SYMBOL.symbol) } as usize;
        assert!(!is_registered_symbol(counterfeit));
        assert!(!is_registered_symbol(image + 8) && !is_registered_symbol(image + 4));
        // Words that are no cell at all, among them a number's magnitude
        // (bare_receiver's 0x7fff_ffff_f000): rejected without a read.
        for raw in [
            0usize,
            8,
            0x1000,
            crate::value::addr_class::HANDLE_BAND_MAX - 8,
            usize::MAX / 2,
            0x7fff_ffff_f000,
        ] {
            assert!(!is_registered_symbol(raw), "{raw:#x}");
        }
        // A readable-looking word on an inaccessible page: a read would fault.
        #[cfg(unix)]
        unsafe {
            let page = libc::mmap(
                std::ptr::null_mut(),
                4096,
                libc::PROT_NONE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            );
            assert_ne!(page, libc::MAP_FAILED);
            assert!(!is_registered_symbol(page as usize + 64));
            libc::munmap(page, 4096);
        }
    }
}
