//! Object model step 5: the probe's derived key index must keep answering
//! after a copying collection moves the keys its record's list holds. A
//! symbol and a plain string are arena objects that move; the index compares
//! the list's own (rewritten) words, never a copy taken when it was derived.
use super::*;

const STRING_TAG: u64 = 0x7FFF_0000_0000_0000;

unsafe fn probe(obj: usize, key_bits: u64, hash: u64) -> u64 {
    crate::object::layout_probe::perry_probe_get(
        crate::object::layout_probe::test_dir(),
        (obj - perry_abi::RECEIVER_HANDLE_FLOOR) as i64,
        key_bits,
        hash,
    )
    .to_bits()
}

unsafe fn list_word(obj: usize, pos: usize) -> u64 {
    crate::object::layout_probe::test_list_word(obj as *const crate::object::ObjectHeader, pos)
}

#[test]
fn probe_index_answers_symbol_and_plain_string_keys_after_a_copying_minor() {
    let _guard = CopyingNurseryTestGuard::new(2);
    let _trigger_guard = GcTriggerThresholdTestGuard::suppress_automatic_triggers();

    // Twelve plain-string keys (more than the probe scans, so its index is
    // a table) and one symbol, all young.
    let obj = crate::object::js_object_alloc(0, 16);
    js_shadow_slot_set(0, ptr_bits(obj as usize));
    let names: Vec<String> = (0..12).map(|i| format!("probe_moving_key_{i}")).collect();
    for (i, n) in names.iter().enumerate() {
        let key = crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
        let o = (js_shadow_slot_get(0) & POINTER_MASK) as *mut crate::object::ObjectHeader;
        crate::object::js_object_set_field_by_name(o, key, i as f64);
    }
    let desc = crate::string::js_string_from_bytes(b"probe.moving".as_ptr(), 12);
    let sym = unsafe { crate::symbol::js_symbol_new(f64::from_bits(STRING_TAG | desc as u64)) };
    js_shadow_slot_set(1, sym.to_bits());
    let o = js_shadow_slot_get(0) & POINTER_MASK;
    unsafe {
        crate::symbol::js_object_set_symbol_property(f64::from_bits(ptr_bits(o as usize)), sym, 99.0)
    };

    let obj_before = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let sym_before = js_shadow_slot_get(1);
    let sym_id = unsafe {
        (*((sym_before & POINTER_MASK) as *const crate::symbol::SymbolHeader)).id
    };
    let string_word_before = unsafe { list_word(obj_before, 5) };
    assert_eq!(string_word_before & 0xFFFF_0000_0000_0000, STRING_TAG);
    let string_before = (string_word_before & POINTER_MASK) as usize;
    assert!(
        crate::arena::pointer_in_nursery(string_before),
        "the condition under test: the list holds a young (moving) plain string"
    );
    assert!(
        crate::arena::pointer_in_nursery((sym_before & POINTER_MASK) as usize),
        "the condition under test: the symbol is young (moving)"
    );

    // Derive the index: the first probe of the record answers the symbol.
    assert_eq!(unsafe { probe(obj_before, sym_before, sym_id) }, 99f64.to_bits());

    let trace = collect_minor_trace(GcTriggerKind::Direct);
    assert_copied_minor_trace(&trace, true, CopiedMinorFallbackReason::None, false);

    let obj_after = (js_shadow_slot_get(0) & POINTER_MASK) as usize;
    let sym_after = js_shadow_slot_get(1);
    assert_ne!(sym_after, sym_before, "the symbol must have moved");
    let string_after = (unsafe { list_word(obj_after, 5) } & POINTER_MASK) as usize;
    assert_ne!(string_after, string_before, "the plain-string key must have moved");

    // The symbol key: the moved symbol is found; the index holds no copy.
    assert_eq!(unsafe { probe(obj_after, sym_after, sym_id) }, 99f64.to_bits());
    // The plain-string key: an atom of its text created after the move
    // matches the moved list word by text.
    let n = names[5].as_bytes();
    let text = crate::string::js_string_from_bytes(n.as_ptr(), n.len() as u32);
    let atom = unsafe { crate::string::property_atom(text) };
    let hash = unsafe { crate::string::property_atom_hash(atom) };
    let got = unsafe { probe(obj_after, STRING_TAG | atom as u64, hash) };
    assert_eq!(got, 5f64.to_bits(), "the moved plain-string key answers by text");
}
