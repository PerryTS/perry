//! Executable literals are data, not startup allocations. The linker folds
//! short names by their complete byte spelling, without a runtime index.
use crate::{module::LlModule, strings::StringEntry};

const GC_TYPE_STRING: u8 = 3;
const IMAGE_GC_FLAGS: u8 = 0x15;
const STRING_IMAGE_MARKER: u16 = 0x494d;

pub(super) fn name(entry: &StringEntry) -> String {
    if entry.byte_len <= 64 {
        let spelling: String = entry.bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "perry_image_{}_{}_{}",
            "literal",
            u8::from(entry.is_wtf8),
            spelling
        )
    } else {
        format!("{}_image_{}", entry.handle_global, "literal")
    }
}

pub(super) fn pointer(entry: &StringEntry) -> String {
    format!(
        "getelementptr (i8, ptr @{}, i64 {})",
        name(entry),
        crate::target_layout::GC_HEADER_SIZE_BYTES
    )
}

pub(super) fn emit(llmod: &mut LlModule, entry: &StringEntry) -> String {
    let name = name(entry);
    let mut units = 0;
    let mut flags = 4u32; // STRING_FLAG_WTF8_VALIDATED
    let mut previous_lead = false;
    let mut at = 0;
    while at < entry.bytes.len() {
        let lead = entry.bytes[at];
        let width = if lead < 0x80 {
            1
        } else if lead < 0xe0 {
            2
        } else if lead < 0xf0 {
            3
        } else {
            4
        };
        let surrogate = width == 3 && lead == 0xed && entry.bytes[at + 1] >= 0xa0;
        let trail = surrogate && entry.bytes[at + 1] >= 0xb0;
        if surrogate {
            flags |= 1;
        }
        if previous_lead && trail {
            flags &= !4;
        }
        previous_lead = surrogate && !trail;
        units += if width == 4 { 2 } else { 1 };
        at += width;
    }
    // Shared headers have complete encoding proofs and are immutable.
    // These objects never enter an arena or malloc ownership list, so GC
    // cannot reclaim or rewrite them. Both mark and pin are born in data:
    // precise roots may visit these immutable leaves during a full cycle.
    let n = entry.byte_len + 1;
    let ty = format!("{{ i8, i8, i16, i32, i32, i32, i32, i32, i32, [{n} x i8] }}");
    let gc_flags = IMAGE_GC_FLAGS;
    llmod.add_raw_global(format!(
        "@{name} = weak_odr constant {ty} {{ i8 {GC_TYPE_STRING}, i8 {gc_flags}, i16 {STRING_IMAGE_MARKER}, i32 {}, i32 {units}, i32 {}, i32 {}, i32 0, i32 {flags}, [{n} x i8] {} }}, align 8",
        28 + n, entry.byte_len, entry.byte_len, entry.escaped_ir
    ));
    pointer(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::StringPool;

    #[test]
    fn split_units_preserve_cross_module_weak_image_identity() {
        for target in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"] {
            let mut m = LlModule::new(target);
            m.add_raw_global("@image = weak_odr constant i64 1".into());
            m.add_internal_global("holder", crate::types::I64, "ptrtoint (ptr @image to i64)");
            for name in ["first", "second"] {
                let block = m
                    .define_function(name, crate::types::I64, vec![])
                    .create_block("entry");
                let value = block.load(crate::types::I64, "@holder");
                block.ret(crate::types::I64, &value);
            }
            let units = m.render_codegen_units(2);
            assert_eq!(units.len(), 2);
            assert_eq!(
                units
                    .iter()
                    .filter(|u| u.contains("@image = weak_odr constant i64 1"))
                    .count(),
                1
            );
            assert_eq!(
                units
                    .iter()
                    .filter(|u| u.contains("@image = external constant i64"))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn literal_identity_uses_all_bytes_across_modules_and_preserves_encoding() {
        let mut a = StringPool::with_prefix("a".into());
        let mut b = StringPool::with_prefix("b".into());
        let ai = a.intern("a😀");
        let bi = b.intern("a😀");
        let mut am = LlModule::new("x86_64-unknown-linux-gnu");
        let mut bm = LlModule::new("x86_64-unknown-linux-gnu");
        assert_eq!(emit(&mut am, a.entry(ai)), emit(&mut bm, b.entry(bi)));
        assert_eq!(am.to_ir(), bm.to_ir());
        let ir = am.to_ir();
        assert!(ir.contains("i32 3, i32 5, i32 5, i32 0, i32 4"));
        assert!(ir.contains("weak_odr constant"));
        assert!(!ir.contains("call "));
        let wi = a.intern_wtf8(b"\xed\xa0\x80");
        emit(&mut am, a.entry(wi));
        assert!(am.to_ir().contains("i8 21, i16 18765"));
        assert!(am.to_ir().contains("i32 1, i32 3, i32 3, i32 0, i32 5"));
    }
}
