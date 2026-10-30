//! Executable literals are data, not startup allocations. The linker folds
//! short names by their complete byte spelling, without a runtime index.
use crate::{module::LlModule, strings::StringEntry};
use sha2::{Digest, Sha256};

/// Literals up to this many bytes are named by their hex spelling (at most
/// twice as many characters). A longer one is named by its length and its
/// SHA-256: a hex spelling of a multi-megabyte literal (a bundled minified
/// script) made a symbol name ld64 rejects (`name.size() <= maxLength` in
/// `makeSymbolStringInPlace`). Equal bytes still give equal names, so the
/// linker folds them across modules as before; the `s` cannot occur in a
/// hex spelling, so the two forms never meet.
const SPELLED_NAME_MAX_BYTES: usize = 256;

const GC_TYPE_STRING: u8 = 3;
const IMAGE_GC_FLAGS: u8 = 0x15;
const STRING_IMAGE_MARKER: u16 = 0x494d;

pub(super) fn name(entry: &StringEntry) -> String {
    name_of(&entry.bytes, entry.is_wtf8)
}

/// The record symbol of a string with these bytes: spelled in hex up to
/// `SPELLED_NAME_MAX_BYTES`, else named by length and SHA-256.
pub(super) fn name_of(bytes: &[u8], is_wtf8: bool) -> String {
    let spelling: String = if bytes.len() <= SPELLED_NAME_MAX_BYTES {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    } else {
        let digest = Sha256::digest(bytes);
        let hash: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("s{}_{}", bytes.len(), hash)
    };
    format!("perry_image_literal_{}_{}", u8::from(is_wtf8), spelling)
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
    let mut flags = 28u32; // STRING_FLAG_WTF8_VALIDATED
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
    // Object model step 5 (case 2c): a coloured key's image carries its
    // colour in flags bits 16..32 (perry-runtime `string::atom_colour`), so a
    // record's colour masks are one flags load per key at mint.
    flags |= crate::layout_desc::image_colour_flags(&entry.bytes);
    let n = entry.byte_len + 1;
    let ty = format!("{{ i8, i8, i16, i32, i32, i32, i32, i32, i32, [{n} x i8], i64 }}");
    let gc_flags = IMAGE_GC_FLAGS;
    let section = if llmod.target_triple.contains("apple") {
        "__TEXT,__perry_strings"
    } else if llmod.target_triple.contains("windows") {
        ".perrystr$m"
    } else {
        "perry_strings"
    };
    llmod.add_coalesced_global(&name, format!(
        "@{name} = weak_odr constant {ty} {{ i8 {GC_TYPE_STRING}, i8 {gc_flags}, i16 {STRING_IMAGE_MARKER}, i32 {}, i32 {units}, i32 {}, i32 {}, i32 0, i32 {flags}, [{n} x i8] {}, i64 {} }}, section \"{section}\", align 8",
        (28 + n + 7) / 8 * 8 + 8, entry.byte_len, entry.byte_len, entry.escaped_ir, crate::nanbox::i64_literal(entry.dispatch_hash)
    ));
    pointer(entry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strings::StringPool;

    #[test]
    fn split_units_preserve_cross_module_weak_image_identity() {
        for target in [
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut m = LlModule::new(target);
            m.add_coalesced_global("image", "@image = weak_odr constant i64 1".into());
            m.add_internal_global("holder", crate::types::I64, "ptrtoint (ptr @image to i64)");
            for name in ["first", "second"] {
                let block = m
                    .define_function(name, crate::types::I64, vec![])
                    .create_block("entry");
                let value = block.load(crate::types::I64, "@holder");
                block.ret(crate::types::I64, &value);
            }
            let units = m.render_codegen_units(2);
            assert_eq!(
                units
                    .iter()
                    .filter(|u| u.contains("$image = comdat any"))
                    .count(),
                1
            );
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
    fn literals_have_linker_owned_bounds_on_all_targets() {
        for (target, section) in [
            ("x86_64-unknown-linux-gnu", "perry_strings"),
            ("aarch64-apple-darwin", "__TEXT,__perry_strings"),
            ("x86_64-pc-windows-msvc", ".perrystr$m"),
            ("wasm32-wasip2", "perry_strings"),
        ] {
            let mut pool = StringPool::with_prefix("bounds".into());
            let id = pool.intern("literal");
            let mut module = LlModule::new(target);
            emit(&mut module, pool.entry(id));
            let ir = module.to_ir();
            let symbol = name(pool.entry(id));
            assert_eq!(
                ir.contains(&format!("${symbol} = comdat any")),
                !target.contains("apple")
            );
            assert_eq!(
                ir.contains(&format!("comdat(${symbol})")),
                !target.contains("apple")
            );
            assert!(module.to_ir().contains(&format!("section \"{section}\"")));
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
        // units, byte length, capacity, 0, flags: the encoding-proof word is
        // the validated-WTF-8 base (28), plus 1 for a lone surrogate below.
        assert!(ir.contains("i32 3, i32 5, i32 5, i32 0, i32 28"), "{ir}");
        assert!(ir.contains("weak_odr constant"));
        assert!(!ir.contains("call "));
        let wi = a.intern_wtf8(b"\xed\xa0\x80");
        emit(&mut am, a.entry(wi));
        assert!(am.to_ir().contains("i8 21, i16 18765"));
        assert!(am.to_ir().contains("i32 1, i32 3, i32 3, i32 0, i32 29"));
    }

    #[test]
    fn long_literal_names_are_bounded_and_content_addressed() {
        let mut a = StringPool::with_prefix("a".into());
        let mut b = StringPool::with_prefix("b".into());
        let short = a.intern("length");
        assert_eq!(name(a.entry(short)), "perry_image_literal_0_6c656e677468");
        let edge = "x".repeat(SPELLED_NAME_MAX_BYTES);
        let edge_id = a.intern(&edge);
        assert!(name(a.entry(edge_id)).ends_with(&"78".repeat(SPELLED_NAME_MAX_BYTES)));
        let big = "/*! minified */".repeat(400_000);
        let (ai, bi) = (a.intern(&big), b.intern(&big));
        let long = name(a.entry(ai));
        assert!(long.len() < 128, "{} bytes", long.len());
        assert!(long.starts_with(&format!("perry_image_literal_0_s{}_", big.len())));
        assert_eq!(long, name(b.entry(bi)));
        let mut other = big.clone();
        other.replace_range(0..1, "?");
        let oi = a.intern(&other);
        assert_ne!(long, name(a.entry(oi)));
    }
    #[test]
    fn split_comdat_images_ignore_linkage_words_in_literal_bytes() {
        for target in [
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut pool = StringPool::with_prefix("linkage_words".into());
            let id = pool.intern("literal = external thread_local , comdat(");
            let mut m = LlModule::new(target);
            let image = emit(&mut m, pool.entry(id));
            let image_name = name(pool.entry(id));
            m.add_internal_global("holder", crate::types::PTR, &image);
            for function in ["first", "second"] {
                let block = m
                    .define_function(function, crate::types::PTR, vec![])
                    .create_block("entry");
                let value = block.load(crate::types::PTR, "@holder");
                block.ret(crate::types::PTR, &value);
            }
            let units = m.render_codegen_units(2);
            assert_eq!(
                units
                    .iter()
                    .filter(|ir| ir.contains(&format!("@{image_name} = weak_odr constant")))
                    .count(),
                1
            );
            assert_eq!(
                units
                    .iter()
                    .filter(|ir| ir.contains(&format!("@{image_name} = external constant")))
                    .count(),
                1
            );
            assert_eq!(
                units
                    .iter()
                    .filter(|ir| ir.contains(&format!("${image_name} = comdat any")))
                    .count(),
                1
            );
            // Every target's IR ownership is checked above. Emission needs
            // its optional backend to be linked into this test executable.
            #[cfg(feature = "llvm-inprocess")]
            for (index, ir) in units
                .iter()
                .enumerate()
                .filter(|_| !target.starts_with("wasm32") || cfg!(feature = "target-wasi"))
            {
                crate::inprocess::compile_ll_to_object_inprocess(
                    ir,
                    target,
                    &["-O2".into()],
                    &format!("literal-linkage-{index}"),
                    false,
                )
                .expect("literal bytes must not change ownership or COMDAT declarations");
            }
        }
    }
}
