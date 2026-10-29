//! Auxiliary realm cells use the same pointer-slot and GC-header encoding.
use super::*;

/// An object's metadata is a real GC cell. Heap overflow and prototype writes
/// use the normal metadata fields and the image write barrier after startup.
pub(crate) fn emit_metadata_cell(
    module: &mut LlModule,
    name: &str,
    prototype: Slot,
    flags: u64,
    proto_serial: u64,
) -> Vec<TagFixup> {
    emit_metadata_with_expando(module, name, prototype, flags, proto_serial, Slot::Bits(0))
}

/// A function's rare internal state is the ordinary metadata expando edge of
/// its property bag. It uses the same cell and pointer encoding as other meta.
pub(crate) fn emit_metadata_with_expando(
    module: &mut LlModule,
    name: &str,
    prototype: Slot,
    flags: u64,
    proto_serial: u64,
    expando: Slot,
) -> Vec<TagFixup> {
    emit_metadata_record(
        module,
        name,
        prototype,
        flags,
        proto_serial,
        expando,
        Slot::Bits(0),
    )
}

pub(crate) fn emit_metadata_record(
    module: &mut LlModule,
    name: &str,
    prototype: Slot,
    flags: u64,
    proto_serial: u64,
    expando: Slot,
    native_state: Slot,
) -> Vec<TagFixup> {
    use perry_abi::ObjectMeta;
    let mut words = vec![Slot::Bits(0); std::mem::size_of::<ObjectMeta>() / 8];
    words[std::mem::offset_of!(ObjectMeta, prototype) / 8] = prototype;
    words[std::mem::offset_of!(ObjectMeta, flags) / 8] = Slot::Bits(flags);
    words[std::mem::offset_of!(ObjectMeta, proto_serial) / 8] = Slot::Bits(proto_serial);
    words[std::mem::offset_of!(ObjectMeta, expando) / 8] = expando;
    words[std::mem::offset_of!(ObjectMeta, native_state) / 8] = native_state;
    emit_word_record(module, name, perry_abi::GC_TYPE_OBJECT_META, &words)
}

fn emit_word_record(module: &mut LlModule, name: &str, kind: u8, words: &[Slot]) -> Vec<TagFixup> {
    let mut types = Vec::new();
    let mut values = Vec::new();
    let mut fixups = Vec::new();
    for (index, slot) in words.iter().enumerate() {
        let (ty, value, tag) = slot_init(&module.target_triple, slot);
        types.push(ty);
        values.push(format!("{ty} {value}"));
        if let Some(tag) = tag {
            fixups.push(TagFixup {
                cell: name.into(),
                offset: GC_HEADER_SIZE + index as u64 * 8,
                tag,
            });
        }
    }
    let payload = types.join(", ");
    let size = GC_HEADER_SIZE + words.len() as u64 * 8;
    module.add_raw_global(format!(
        "@{name} = internal global {{ i8, i8, i16, i32, {{ {payload} }} }} {{ i8 {kind}, i8 {IMAGE_CELL_FLAGS}, i16 0, i32 {size}, {{ {payload} }} {{ {} }} }}, section \"{}\", align 8",
        values.join(", "), section(&module.target_triple)
    ));
    fixups
}

/// A symbol owns an ordinary image-string description. Its native-storage bit
/// is clear: readers must never interpret an image symbol as a leaked Arc tail.
/// `id` is the identity assigned by the program's symbol birth facts.
pub(crate) fn emit_symbol_cell(
    module: &mut LlModule,
    name: &str,
    description: &str,
    id: u64,
    registered: bool,
) {
    let (pad_ty, pad_init) = if crate::target_layout::target_is_ilp32(&module.target_triple) {
        ("i32, ", "i32 0, ")
    } else {
        ("", "")
    };
    let size = GC_HEADER_SIZE + std::mem::size_of::<perry_abi::SymbolHeader<u8>>() as u64;
    module.add_raw_global(format!(
        "@{name} = global {{ i8, i8, i16, i32, i32, i32, ptr, {pad_ty}i64 }} {{ i8 {}, i8 {IMAGE_CELL_FLAGS}, i16 0, i32 {size}, i32 {}, i32 {}, ptr {}, {pad_init}i64 {} }}, section \"{}\", align 8",
        perry_abi::GC_TYPE_SYMBOL,
        perry_abi::SYMBOL_HEADER_MAGIC,
        u32::from(registered),
        user_address(description),
        crate::nanbox::i64_literal(id),
        section(&module.target_triple)
    ));
}

/// The ordinary four-word accessor pair, not a canonical/shared array.
/// Absent JS halves are undefined; absent raw body entries are zero.
pub(crate) fn emit_accessor_pair(
    module: &mut LlModule,
    name: &str,
    get: Option<&str>,
    set: Option<&str>,
) -> Vec<TagFixup> {
    use perry_abi::*;
    let mut slots = vec![Slot::Bits(0); ACCESSOR_PAIR_LEN];
    for (index, function) in [(ACCESSOR_PAIR_GET, get), (ACCESSOR_PAIR_SET, set)] {
        slots[index] = function.map_or(Slot::Bits(crate::nanbox::TAG_UNDEFINED), |symbol| {
            Slot::Cell(symbol.into(), POINTER_TAG)
        });
    }
    emit_array_cell_with_fields(
        module,
        name,
        &ArrayCell {
            exported: false,
            shared: false,
            reserved: if get.is_none() && set.is_none() {
                GC_LAYOUT_POINTER_FREE
            } else {
                0
            },
            length: ACCESSOR_PAIR_LEN as u32,
            capacity: ACCESSOR_PAIR_LEN as u32,
            dense_prefix: 0,
            named: None,
            slots: &slots,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_symbols_and_raw_byte_captures_have_target_layouts() {
        assert_eq!(std::mem::size_of::<perry_abi::SymbolHeader<u8>>(), 24);
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(triple);
            module.add_raw_global("@description = external global i8".into());
            emit_symbol_cell(&mut module, "symbol", "description", 42, false);
            let fixups = emit_metadata_cell(
                &mut module,
                "meta",
                Slot::Cell("symbol".into(), POINTER_TAG),
                32,
                17,
            );
            let raw = Slot::Address {
                symbol: "description".into(),
                offset: 28,
                tag: 0,
            };
            let (_, pointer, fixup) = slot_init(triple, &raw);
            assert!(pointer.contains("@description, i64 28"));
            assert_eq!(fixup, None);
            let ir = module.to_ir();
            assert!(ir.contains("i8 32, i8 5, i16 0, i32 32"));
            assert!(ir.contains("i8 19, i8 5, i16 0, i32 168"));
            if triple.contains("apple") {
                assert_eq!(fixups.len(), 1);
                assert_eq!(fixups[0].offset, GC_HEADER_SIZE);
            } else {
                assert!(fixups.is_empty());
            }
            let context = inkwell::context::Context::create();
            let parsed = crate::inprocess::parse_ir_text(&context, &ir, "realm_records").unwrap();
            parsed.verify().unwrap();
        }
    }
}
