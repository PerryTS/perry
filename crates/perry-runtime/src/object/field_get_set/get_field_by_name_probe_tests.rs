//! Probe-dispatch invariants for the by-name property-get slow tail (#7867).
//!
//! Buffer and small TypedArray allocations can be headerless, so their
//! registry probes must stay ahead of the first `GcHeader` read. Sets cannot:
//! every registered Set is `GC_TYPE_SET`. Symbols straddle both storage
//! classes, but every one carries `SYMBOL_MAGIC` in its own first word. These
//! tests pin the resulting split instead of merely checking the read result.

use super::*;

fn key(bytes: &[u8]) -> *const crate::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn leaked_symbol(description: &str) -> usize {
    let description = crate::string::js_string_from_str(description);
    let description = f64::from_bits(crate::value::js_nanbox_string(description as i64).to_bits());
    let bits = unsafe { crate::symbol::js_symbol_for(description) }.to_bits();
    let ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(bits)) as usize;
    assert_ne!(ptr, 0, "test premise: Symbol.for allocated a symbol");
    assert!(
        crate::symbol::is_registered_symbol(ptr),
        "test premise: the leaked symbol is registered on this thread"
    );
    ptr
}

fn fresh_symbol(description: &str) -> usize {
    let description = crate::string::js_string_from_str(description);
    let description = f64::from_bits(crate::value::js_nanbox_string(description as i64).to_bits());
    let bits = unsafe { crate::symbol::js_symbol_new(description) }.to_bits();
    let ptr = crate::value::js_nanbox_get_pointer(f64::from_bits(bits)) as usize;
    assert_ne!(
        ptr, 0,
        "test premise: Symbol(description) allocated a symbol"
    );
    assert!(
        crate::symbol::is_registered_symbol(ptr),
        "test premise: the fresh symbol is registered on this thread"
    );
    ptr
}

fn tail(addr: usize, property: &[u8]) -> JSValue {
    get_field_by_name_object_tail(addr as *const ObjectHeader, key(property))
}

#[test]
fn plain_object_miss_skips_set_and_symbol_registries() {
    leaked_symbol("perry-7867-arm-symbol");
    let _set = crate::set::js_set_alloc(4);
    let object = crate::object::js_object_alloc(0, 0) as usize;

    // Warm unrelated lazy state before taking the counters.
    assert!(tail(object, b"missing").is_undefined());
    let set_before = crate::set::test_set_registry_probe_count();
    let symbol_before = crate::symbol::test_symbol_registry_probe_count();

    assert!(tail(object, b"missing").is_undefined());
    assert_eq!(
        crate::set::test_set_registry_probe_count(),
        set_before,
        "GC_TYPE_OBJECT rules a Set out; a property miss must not probe SET_REGISTRY"
    );
    assert_eq!(
        crate::symbol::test_symbol_registry_probe_count(),
        symbol_before,
        "a plain-object property miss must not take the process-global symbol mutex"
    );

    // Sabotage the cheap SYMBOL_MAGIC screen. The authoritative registry probe
    // must return, while the answer and the Set counter stay unchanged.
    let restore = crate::symbol::test_disable_symbol_magic_screen(true);
    let set_before = crate::set::test_set_registry_probe_count();
    let symbol_before = crate::symbol::test_symbol_registry_probe_count();
    let answer = tail(object, b"missing");
    let symbol_after = crate::symbol::test_symbol_registry_probe_count();
    crate::symbol::test_disable_symbol_magic_screen(restore);

    assert!(answer.is_undefined());
    assert!(
        symbol_after > symbol_before,
        "with the magic screen defeated the symbol registry probe must return"
    );
    assert_eq!(
        crate::set::test_set_registry_probe_count(),
        set_before,
        "defeating the symbol screen must not reintroduce the Set probe"
    );
}

#[test]
fn set_and_both_symbol_storage_classes_still_dispatch() {
    let set = crate::set::js_set_alloc(4) as usize;
    let set_before = crate::set::test_set_registry_probe_count();
    let size = tail(set, b"size");
    assert_eq!(f64::from_bits(size.bits()), 0.0);
    assert!(
        crate::set::test_set_registry_probe_count() > set_before,
        "GC_TYPE_SET must still enter the authoritative Set registry"
    );

    let symbol = leaked_symbol("perry-7867-headerless-symbol");
    assert!(
        unsafe { crate::symbol::may_be_symbol_header(symbol as *const u8) },
        "a Box-leaked symbol must carry SYMBOL_MAGIC without a GcHeader"
    );
    let symbol_before = crate::symbol::test_symbol_registry_probe_count();
    let description = tail(symbol, b"description");
    assert!(description.is_string());
    assert!(
        crate::symbol::test_symbol_registry_probe_count() > symbol_before,
        "a possible Symbol must still enter the authoritative symbol registry"
    );

    let symbol = fresh_symbol("perry-7867-gc-symbol");
    assert!(
        unsafe { crate::symbol::may_be_symbol_header(symbol as *const u8) },
        "a GC-allocated symbol must carry SYMBOL_MAGIC too"
    );
    let symbol_before = crate::symbol::test_symbol_registry_probe_count();
    let description = tail(symbol, b"description");
    assert!(description.is_string());
    assert!(
        crate::symbol::test_symbol_registry_probe_count() > symbol_before,
        "a GC-allocated Symbol must still enter the authoritative symbol registry"
    );
}
