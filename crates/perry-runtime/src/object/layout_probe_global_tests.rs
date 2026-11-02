//! DEC 243(5): globalThis is an ordinary object whose builtin names are
//! atoms, so the probe word of its record proves absence: a miss on the
//! global object continues the walk instead of declining to the by-name
//! ladder.
use super::*;

#[test]
fn global_object_keys_are_atoms_and_a_miss_proves_absence() {
    std::thread::Builder::new()
        .stack_size(16 << 20)
        .spawn(|| unsafe {
            crate::gc::initialize_runtime_fixture();
            let _no_move = crate::gc::GcSuppressScope::new();
            let global =
                crate::JSValue::from_bits(crate::object::js_get_global_this().to_bits());
            assert!(global.is_pointer());
            let global = global.as_pointer::<ObjectHeader>();
            let record = record_of(test_dir(), perry_abi::LayoutWord((*global).parent_class_id));
            assert!(!record.is_null() && (*record).present());
            let count = (*record).logical_key_count as usize;
            assert!(count > 100, "the global object lists its builtin names: {count}");
            let keys = (*record).keys as usize as *const crate::array::ArrayHeader;
            let words = crate::array::array_elements_ptr(keys) as *const u64;
            let mut not_atoms = Vec::new();
            let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
            for i in 0..count {
                let bits = *words.add(i);
                if bits == TAG_HOLE {
                    continue;
                }
                // A builtin name is the runtime's static image atom, in the
                // string image section. The missing-atom fallback of
                // `js_string_pool_atom` also makes image-MARKED strings, but
                // allocates them, so the marker alone would pass a name
                // `global_atoms.rs` does not declare.
                if key_word_hash(bits).is_some()
                    && (bits & TAG_MASK != STRING_TAG
                        || crate::value::addr_class::in_string_image_section_for_tests(
                            (bits & POINTER_MASK) as usize,
                        ))
                {
                    continue;
                }
                let text =
                    crate::string::js_string_key_bytes(crate::JSValue::from_bits(bits), &mut sso)
                        .map(|t| String::from_utf8_lossy(t).into_owned())
                        .unwrap_or_else(|| format!("{bits:#x}"));
                not_atoms.push(text);
            }
            assert!(not_atoms.is_empty(), "global keys that are not image atoms: {not_atoms:?}");
            let word = derive_word(&*record);
            assert_eq!(
                word & (PROBE_READY | PROBE_DECLINE | PROBE_ATOMS),
                PROBE_READY | PROBE_ATOMS,
                "a miss on the global object must prove absence (word {word:#x})"
            );
        })
        .unwrap()
        .join()
        .unwrap();
}
