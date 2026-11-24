//! Symbol constructor + value FFI entry points: `Symbol()`, `Symbol(desc)`,
//! `Symbol.for`, `Symbol.keyFor`, `sym.description`, `sym.toString()`,
//! `typeof sym`, and symbol equality.

use super::*;
use crate::string::{js_string_from_bytes, StringHeader};
use std::collections::HashMap;

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
    let desc_ptr: *mut StringHeader = if bits == TAG_UNDEFINED {
        // `Symbol()` — no description.
        std::ptr::null_mut()
    } else if tag == STRING_TAG {
        (bits & POINTER_MASK) as *mut StringHeader
    } else {
        // Spec step 2 (sec-symbol-constructor): descString = ToString(description).
        // ToString rejects a Symbol with a TypeError (test262 desc-to-string-symbol);
        // objects/numbers/booleans coerce, running `toString`/`valueOf`
        // (test262 desc-to-string). `js_string_coerce` is the full ToString.
        if js_is_symbol(description_f64) != 0 {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
        }
        crate::builtins::js_string_coerce(description_f64) as *mut StringHeader
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
    // Resolve the registry key string. Spec step 1 (sec-symbol.for):
    // `key = ToString(key)`. A NaN-boxed heap string is used directly, as is a
    // raw StringHeader pointer from legacy callsites; every other value is
    // ToString-coerced so `Symbol.for(undefined)` registers under "undefined",
    // `Symbol.for(42)` under "42", `Symbol.for(null)`/`true` likewise — and
    // `Symbol.for(x) === Symbol.for(String(x))`. A Symbol key throws a
    // TypeError, matching `ToString(symbol)` (and `Symbol()`'s coercion).
    let key_ptr = if tag == STRING_TAG {
        (bits & POINTER_MASK) as *const StringHeader
    } else if crate::value::addr_class::is_plausible_heap_addr(bits as usize)
        && crate::value::addr_class::try_read_tracked_gc_header_of_type(
            bits as usize,
            crate::gc::GC_TYPE_STRING,
        )
        .is_some()
    {
        // A raw StringHeader pointer from a legacy callsite needs allocation
        // AND type evidence before it is dereferenced. Magnitude alone is not
        // evidence: a subnormal double's bits can land in the heap range (the
        // guard below documents the fixture), and reading it as a StringHeader
        // is a read of unowned memory. Without the evidence the key falls
        // through to ToString, which is what Node does for any number.
        bits as *const StringHeader
    } else {
        if js_is_symbol(key_f64) != 0 {
            crate::collection_iter::throw_type_error("Cannot convert a Symbol value to a string");
        }
        // `js_string_coerce` is the full ToString: undefined→"undefined",
        // null→"null", numbers/bools/objects coerce, and short (SSO) strings
        // materialize onto the heap so `str_from_header` can read them.
        crate::builtins::js_string_coerce(key_f64) as *const StringHeader
    };
    let key = match str_from_header(key_ptr) {
        Some(s) => s,
        None => return f64::from_bits(TAG_UNDEFINED),
    };

    f64::from_bits(POINTER_TAG | (symbol_for_text(&key) as u64 & POINTER_MASK))
}

/// `Symbol.<name>`: `number` is its index in `WELL_KNOWN_SYMBOLS`.
#[no_mangle]
pub extern "C" fn js_well_known_symbol(number: i32) -> f64 {
    let symbol = well_known_symbol(perry_abi::WELL_KNOWN_SYMBOLS[number as usize]);
    f64::from_bits(POINTER_TAG | (symbol as u64 & POINTER_MASK))
}

/// The symbol a class member declaration spells (perry-hir's
/// `Expr::known_symbol`): `@@__perry_wk_<name>` or a registry key.
pub(crate) fn declared_symbol(spelling: &str) -> usize {
    match spelling.strip_prefix(WK_PREFIX) {
        Some(name) => well_known_symbol(name) as usize,
        None => symbol_for_text(spelling),
    }
}

/// The registry's symbol for `key` (created on first use).
fn symbol_for_text(key: &str) -> usize {
    let mut guard = SYMBOL_REGISTRY.lock().unwrap();
    if guard.is_none() {
        *guard = Some(HashMap::new());
    }
    let registry = guard.as_mut().unwrap();
    if let Some(&ptr_usize) = registry.get(key) {
        return ptr_usize;
    }

    // Not found — allocate a persistent SymbolHeader. We use Box::leak so the
    // pointer outlives any GC cycle (the registry holds it as a root). The
    // description text is stored in the symbol's native text as a
    // process-lifetime Arc<str>; the header's `description` pointer stays
    // null. Readers (`sym.description`, `sym.toString()`, key_for) consult
    // the side table and materialize a StringHeader in *their own* arena on
    // demand, so cross-thread reads are safe even when the originating
    // worker's arena was torn down.
    let sym_ptr = leak_symbol(SymbolHeader {
        magic: SYMBOL_MAGIC,
        registered: 1,
        description: std::ptr::null_mut(),
        id: next_id(),
    });
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::SymbolGlobal,
        );
    }
    // Fully initialize the side tables BEFORE publishing the pointer in
    // the registry. Otherwise a concurrent `Symbol.for("same_key")` on
    // another thread can see the pointer via the registry but get None
    // from registered_symbol_description, returning a transiently bogus
    // sym.description / sym.toString() / Symbol.keyFor(). Lock order is
    // SYMBOL_REGISTRY owns the symbol and its native text;
    // no reader takes them in the reverse order.
    record_registered_symbol_description(sym_ptr as usize, key);
    registry.insert(key.to_string(), sym_ptr as usize);
    drop(guard);
    sym_ptr as usize
}

/// #6676: a *computed* read of a well-known symbol off the `Symbol`
/// constructor — `Symbol[name]` with a runtime key. Dot access (`Symbol.iterator`)
/// and a string-literal bracket key are folded at HIR, but a dynamic key can't be folded. This is the exact shape esbuild's
/// `__knownSymbol` helper emits when downleveling `yield*`/`async` to
/// es2015/es2017: `(symbol = Symbol[name]) ? symbol : Symbol.for("Symbol." + name)`.
///
/// Map a well-known member name to the cached well-known symbol — identity-equal
/// to the dot form, so `Symbol[name] === Symbol.iterator`. Any other key (a
/// non-string, or a name that isn't a well-known symbol) falls back to the
/// ordinary `Symbol[key]` read on the constructor value, making this a strict
/// superset of the prior behavior (a non-well-known key still reads `undefined`,
/// which is exactly what lets the `__knownSymbol` fallback to `Symbol.for` fire).
#[no_mangle]
pub unsafe extern "C" fn js_symbol_computed_member(ctor_f64: f64, key_f64: f64) -> f64 {
    // A heap or inline SSO string key (#11519) — `"match"` and `"split"` are
    // short enough to arrive inline when built at runtime.
    let well_known = crate::string::with_string_value_bytes(key_f64, |bytes| {
        std::str::from_utf8(bytes)
            .ok()
            .filter(|name| is_well_known_symbol_member_name(name))
            .map(str::to_string)
    })
    .flatten();
    if let Some(name) = well_known {
        let wk_ptr = well_known_symbol(&name);
        return f64::from_bits(POINTER_TAG | (wk_ptr as u64 & POINTER_MASK));
    }
    // Not a well-known symbol name — behave exactly like a plain `Symbol[key]`
    // read on the constructor value.
    crate::value::js_dyn_index_get(ctor_f64, key_f64)
}

/// The well-known-symbol member names surfaced on the `Symbol` constructor. Kept
/// in sync with `is_well_known_symbol_member` in perry-hir's `expr_member.rs`
/// (the dot / string-literal fold) so all three forms — `Symbol.iterator`,
/// `Symbol["iterator"]`, `Symbol[name]` — resolve to the same cached symbol.
fn is_well_known_symbol_member_name(name: &str) -> bool {
    matches!(
        name,
        "toPrimitive"
            | "hasInstance"
            | "toStringTag"
            | "species"
            | "match"
            | "matchAll"
            | "replace"
            | "search"
            | "split"
            | "isConcatSpreadable"
            | "unscopables"
            | "iterator"
            | "asyncIterator"
            | "dispose"
            | "asyncDispose"
    )
}

// #1561-style force-keep: `js_symbol_computed_member` has no internal Rust
// callers — only generated IR (perry-hir lowers `Symbol[key]` to a call to it),
// so LTO / whole-program-bitcode link modes are free to internalize and
// dead-strip it. The `#[used]` reference edge keeps the export alive.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_SYMBOL_COMPUTED_MEMBER: unsafe extern "C" fn(f64, f64) -> f64 =
    js_symbol_computed_member;

#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_WELL_KNOWN_SYMBOL: extern "C" fn(i32) -> f64 = js_well_known_symbol;

/// `Symbol.keyFor(sym)` — reverse lookup. Returns the registration key as a
/// string for registered symbols, or undefined for non-registered symbols.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_key_for(sym_f64: f64) -> f64 {
    // Spec step 1 (sec-symbol.keyfor): if Type(sym) is not Symbol, throw a
    // TypeError — distinct from the `undefined` returned for a real-but-
    // unregistered symbol below (test262 keyFor/arg-non-symbol).
    if js_is_symbol(sym_f64) == 0 {
        crate::collection_iter::throw_type_error("Symbol.keyFor requires a symbol argument");
    }
    let bits = sym_f64.to_bits();
    let sym_ptr = (bits & POINTER_MASK) as *const SymbolHeader;
    // Well-known symbols (Symbol.toPrimitive, etc.) are NOT in the registry.
    if is_well_known_symbol(sym_ptr as usize) {
        return f64::from_bits(TAG_UNDEFINED);
    }
    if (*sym_ptr).registered & 1 == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Registered symbols carry the description as Arc<str> in the side
    // table; materialize a fresh StringHeader in this thread's arena.
    // #7246: descriptions live off the GC heap now (registered ones in the
    // process-global map, fresh ones in the id-keyed thread-local one), so
    // every reader materializes a fresh StringHeader in the CALLER's arena.
    let Some(s) = symbol_description_text(sym_ptr) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let header = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(STRING_TAG | (header as u64 & POINTER_MASK))
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
    // #7246: descriptions live off the GC heap now (registered ones in the
    // process-global map, fresh ones in the id-keyed thread-local one), so
    // every reader materializes a fresh StringHeader in the CALLER's arena.
    let Some(s) = symbol_description_text(sym_ptr) else {
        return f64::from_bits(TAG_UNDEFINED);
    };
    let header = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(STRING_TAG | (header as u64 & POINTER_MASK))
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
    // Lossy only for the rendered `Symbol(...)` form, matching what
    // `str_from_header(..).unwrap_or_default()` produced before (#7246): a
    // WTF-8 description could never be formatted into a Rust `String` losslessly.
    let desc_str = symbol_description_text(sym_ptr)
        .map(|s| String::from_utf8_lossy(s.as_ref()).into_owned())
        .unwrap_or_default();
    let rendered = format!("Symbol({})", desc_str);
    js_string_from_bytes(rendered.as_ptr(), rendered.len() as u32) as i64
}

/// Return the `typeof` string for a symbol value: "symbol".
/// Codegen can call this in the runtime type-tag dispatch.
#[no_mangle]
pub unsafe extern "C" fn js_symbol_typeof() -> *mut StringHeader {
    let s = b"symbol";
    js_string_from_bytes(s.as_ptr(), s.len() as u32)
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
mod registry_key_tests {
    use super::*;

    #[test]
    fn a_registry_key_never_names_a_well_known_symbol() {
        for (number, name) in perry_abi::WELL_KNOWN_SYMBOLS.iter().enumerate() {
            let spelling = format!("{WK_PREFIX}{name}");
            unsafe {
                let text = js_string_from_bytes(spelling.as_ptr(), spelling.len() as u32);
                let registered = js_symbol_for(crate::value::js_nanbox_string(text as i64));
                let well_known = js_well_known_symbol(number as i32);
                assert_ne!(registered.to_bits(), well_known.to_bits(), "{name}");
                assert_eq!(js_symbol_key_for(well_known).to_bits(), TAG_UNDEFINED);
                assert_ne!(js_symbol_key_for(registered).to_bits(), TAG_UNDEFINED);
                assert_eq!(
                    declared_symbol(&spelling) as u64,
                    well_known.to_bits() & POINTER_MASK
                );
            }
        }
    }

    /// `Symbol.for(<number>)` registers under `ToString(number)`. A subnormal
    /// double's bits can land inside the heap range, and magnitude alone used to
    /// be enough to read them as a raw `StringHeader` — a read of unowned
    /// memory, whose result depends on whatever the allocator left there. The
    /// fixture asserts both halves of the hazard: the bits are heap-plausible,
    /// and no tracked allocation owns that address.
    #[test]
    fn heap_plausible_number_keys_are_coerced_without_dereferencing_their_bits() {
        unsafe {
            for number in [1e-310, f64::from_bits(0x4000_0000)] {
                assert!(
                    crate::value::addr_class::is_plausible_heap_addr(number.to_bits() as usize),
                    "fixture {number} is not heap-plausible: the test would prove nothing"
                );
                assert!(
                    crate::value::addr_class::try_read_tracked_gc_header(number.to_bits() as usize)
                        .is_none(),
                    "fixture {number} must name an unowned address"
                );
                let text = crate::builtins::js_string_coerce(number);
                let expected = js_symbol_for(crate::value::js_nanbox_string(text as i64));
                let actual = js_symbol_for(number);
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "Symbol.for({number}) must register under its ToString text"
                );
                assert_ne!(js_is_symbol(actual), 0);
            }
        }
    }

    /// The other half of the branch must keep working: a raw `StringHeader`
    /// from a legacy callsite (an owned, tracked string allocation) is still
    /// used directly, and shares its registry entry with the boxed string.
    #[test]
    fn tracked_raw_string_pointers_still_share_the_registry_entry() {
        unsafe {
            let bytes = b"legacy raw symbol registry key";
            let header = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
            assert!(
                crate::value::addr_class::try_read_tracked_gc_header_of_type(
                    header as usize,
                    crate::gc::GC_TYPE_STRING,
                )
                .is_some(),
                "a freshly allocated string must carry string type evidence"
            );
            let boxed = js_symbol_for(crate::value::js_nanbox_string(header as i64));
            let raw = js_symbol_for(f64::from_bits(header as u64));
            assert_eq!(raw.to_bits(), boxed.to_bits());
        }
    }
}
