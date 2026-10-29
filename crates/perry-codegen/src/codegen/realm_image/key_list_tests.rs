use super::*;

#[test]
fn resolved_symbol_keys_and_attributes_are_link_time_data() {
    for triple in [
        "x86_64-unknown-linux-gnu",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "wasm32-wasip2",
    ] {
        let mut module = LlModule::new(triple);
        module.add_raw_global("@text = external global i8".into());
        module.add_raw_global("@symbol = external global i8".into());
        let keys = [
            ImageKeySlot {
                value: Slot::ImageString("text".into()),
                string_hash: Some(0xb123_4567_89ab_cdef),
                attrs: 6,
                slot: 1,
            },
            ImageKeySlot {
                value: Slot::Cell("symbol".into(), POINTER_TAG),
                string_hash: None,
                attrs: 8 | 16,
                slot: 0,
            },
        ];
        let (address, fixups) = emit_image_key_list(&mut module, "keys", &keys);
        assert_eq!(address, "getelementptr (i8, ptr @keys, i64 8)");
        let ir = module.to_ir();
        // Attribute array + key list; the symbol is never an undefined slot.
        assert_eq!(ir.matches(" = internal global ").count(), 2);
        assert!(!ir.contains(crate::nanbox::TAG_UNDEFINED_I64));
        assert!(ir.contains("getelementptr (i8, ptr @symbol, i64 8)"));
        let a = perry_abi::canonical_key_attribute_word(0, 6, 1 << 11, Some(1));
        let b = perry_abi::canonical_key_attribute_word(a, 24, 0, Some(0));
        for word in [a, b] {
            let (_, initializer, _) = slot_init(triple, &Slot::Bits(word));
            assert!(ir.contains(&initializer));
        }
        if triple.contains("apple") {
            assert_eq!(fixups.len(), 3);
            assert_eq!(
                fixups.iter().map(|f| f.offset).collect::<Vec<_>>(),
                [32, 40, 48]
            );
            assert_eq!(
                fixups.iter().map(|f| f.tag).collect::<Vec<_>>(),
                [POINTER_TAG, STRING_TAG, POINTER_TAG]
            );
        } else {
            assert!(fixups.is_empty());
        }
        let context = inkwell::context::Context::create();
        let parsed = crate::inprocess::parse_ir_text(&context, &ir, "image_symbol_keys").unwrap();
        parsed.verify().unwrap();
    }
}

#[test]
fn empty_key_list_has_no_cell_or_relocation() {
    let mut module = LlModule::new("x86_64-unknown-linux-gnu");
    let (address, fixups) = emit_image_key_list(&mut module, "empty", &[]);
    assert_eq!(address, "null");
    assert!(fixups.is_empty());
    assert!(!module.to_ir().contains("@empty"));
}
