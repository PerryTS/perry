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
    } else if crate::value::addr_class::is_plausible_heap_addr(bits as usize) {
        // Raw StringHeader pointer from a legacy callsite. Use the canonical
        // heap-address predicate rather than a duplicated literal range so a
        // small-magnitude (e.g. denormal) double key isn't misclassified as a
        // pointer — it falls through to ToString below and coerces correctly.
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

/// #6676: a *computed* read of a well-known symbol off the `Symbol`
/// constructor — `Symbol[name]` with a runtime key. Dot access (`Symbol.iterator`)
/// and a string-literal bracket key are folded to the `@@__perry_wk_` sentinel at
/// HIR, but a dynamic key can't be folded. This is the exact shape esbuild's
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
    let bits = key_f64.to_bits();
    if bits & 0xFFFF_0000_0000_0000 == STRING_TAG {
        let key_ptr = (bits & POINTER_MASK) as *const StringHeader;
        if let Some(name) = str_from_header(key_ptr) {
            if is_well_known_symbol_member_name(&name) {
                let wk_ptr = well_known_symbol(&name);
                return f64::from_bits(POINTER_TAG | (wk_ptr as u64 & POINTER_MASK));
            }
        }
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
#[used]
static KEEP_JS_SYMBOL_COMPUTED_MEMBER: unsafe extern "C" fn(f64, f64) -> f64 =
    js_symbol_computed_member;

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
    if (*sym_ptr).registered == 0 {
        return f64::from_bits(TAG_UNDEFINED);
    }
    // Registered symbols carry the description as Arc<str> in the side
    // table; materialize a fresh StringHeader in this thread's arena.
    if let Some(s) = registered_symbol_description(sym_ptr as usize) {
        let header = js_string_from_bytes(s.as_bytes().as_ptr(), s.len() as u32);
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
        let header = js_string_from_bytes(s.as_bytes().as_ptr(), s.len() as u32);
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
