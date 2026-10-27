//! The realm image's cells as data (WRT, DESIGN §2.1): GC-headed objects and
//! function objects emitted into the writable `perry_realm` section exactly as
//! the heap would hold them, with `MARKED|PINNED` headers and addresses the
//! linker fills. The runtime classifies them by the section's range
//! (`value::addr_class::realm`) and the collector treats them as immortal
//! roots (`gc::image_cells`).
//!
//! A slot naming another image cell holds a NaN-boxed pointer. How it becomes
//! data depends on what the object format can carry:
//!
//! * ELF and COFF: one link-time constant, `ptrtoint + tag`; the load-time
//!   relocation adds the slide to the whole word.
//! * 32-bit targets (wasm32, arm64_32): two 32-bit halves, the address and the
//!   tag's high word. Only the address half is relocated.
//! * 64-bit Apple targets: a chained fixup has no room for a 16-bit tag above
//!   its target, so the slot holds the untagged address (an ordinary rebase)
//!   and `main` ORs the tag in before any initializer runs ([`TagFixup`]),
//!   the way `string_pool` boxes image string handles on the same targets.
//!
//! Step 1 emits only the test fixture ([`test_fixture_requested`]); step 3
//! emits the program's realm through the same cell emitters.
use crate::block::LlBlock;
use crate::module::LlModule;
use crate::types::{DOUBLE, I64, I8, VOID};

const GC_TYPE_OBJECT: u8 = 2;
const GC_TYPE_CLOSURE: u8 = 4;
/// `GC_FLAG_MARKED | GC_FLAG_PINNED`: an image cell is never white and never
/// moves (perry-runtime `realm::IMAGE_CELL_FLAGS`).
const IMAGE_CELL_FLAGS: u8 = 0x01 | 0x04;
const GC_HEADER_SIZE: u64 = crate::target_layout::GC_HEADER_SIZE_BYTES;
const POINTER_TAG: u64 = crate::nanbox::POINTER_TAG;

/// The section name per object format (same scheme as `perry_strings`).
pub(crate) fn section(triple: &str) -> &'static str {
    if triple.contains("apple") {
        "__DATA,__perry_realm"
    } else if triple.contains("windows") {
        ".perryrealm$m"
    } else {
        "perry_realm"
    }
}

/// A value slot of an image object.
#[derive(Clone, Debug)]
pub(crate) enum Slot {
    /// A primitive's bits (undefined, booleans, numbers).
    Bits(u64),
    /// A NaN-boxed pointer to the image cell with this symbol.
    Cell(String),
}

/// A slot `main` must tag before the program runs (64-bit Apple targets).
#[derive(Clone, Debug)]
pub(crate) struct TagFixup {
    pub(crate) cell: String,
    pub(crate) offset: u64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PointerSlots {
    TaggedRelocation,
    SplitHalves,
    TaggedAtStartup,
}

fn pointer_slots(triple: &str) -> PointerSlots {
    if crate::target_layout::target_is_ilp32(triple) {
        PointerSlots::SplitHalves
    } else if triple.contains("apple") {
        PointerSlots::TaggedAtStartup
    } else {
        PointerSlots::TaggedRelocation
    }
}

fn user_address(cell: &str) -> String {
    format!("getelementptr (i8, ptr @{cell}, i64 {GC_HEADER_SIZE})")
}

/// One 8-byte slot as `(llvm type, initializer)`, plus whether `main` must tag
/// it.
fn slot_init(triple: &str, slot: &Slot) -> (&'static str, String, bool) {
    let mode = pointer_slots(triple);
    match (slot, mode) {
        (Slot::Bits(bits), PointerSlots::SplitHalves) => (
            "{ i32, i32 }",
            format!(
                "{{ i32 {}, i32 {} }}",
                *bits as u32 as i32,
                (*bits >> 32) as u32 as i32
            ),
            false,
        ),
        (Slot::Bits(bits), _) => ("i64", crate::nanbox::i64_literal(*bits), false),
        (Slot::Cell(cell), PointerSlots::SplitHalves) => (
            "{ i32, i32 }",
            format!(
                "{{ i32 ptrtoint (ptr {} to i32), i32 {} }}",
                user_address(cell),
                (POINTER_TAG >> 32) as u32 as i32
            ),
            false,
        ),
        (Slot::Cell(cell), PointerSlots::TaggedRelocation) => (
            "i64",
            format!(
                "add (i64 ptrtoint (ptr {} to i64), i64 {})",
                user_address(cell),
                crate::nanbox::i64_literal(POINTER_TAG)
            ),
            false,
        ),
        (Slot::Cell(cell), PointerSlots::TaggedAtStartup) => (
            "i64",
            format!("ptrtoint (ptr {} to i64)", user_address(cell)),
            true,
        ),
    }
}

/// A function object cell: `GcHeader` + `ClosureHeader` with no captures, its
/// `info` the runtime's static `JsFunctionInfo` named `info_symbol` (declared
/// here as an external byte: only its address is used).
pub(crate) fn emit_closure_cell(llmod: &mut LlModule, name: &str, info_symbol: &str, shape: u32) {
    let triple = llmod.target_triple.clone();
    if !llmod.is_declared(info_symbol) {
        llmod.add_raw_global(format!("@{info_symbol} = external global i8"));
    }
    let size = GC_HEADER_SIZE + crate::target_layout::closure_header_size_bytes(&triple);
    llmod.add_raw_global(format!(
        "@{name} = internal global {{ i8, i8, i16, i32, i32, i32, ptr, ptr }} \
         {{ i8 {GC_TYPE_CLOSURE}, i8 {IMAGE_CELL_FLAGS}, i16 0, i32 {size}, i32 0, i32 {}, ptr @{info_symbol}, ptr null }}, \
         section \"{}\", align 8",
        shape as i32,
        section(&triple)
    ));
}

/// An ordinary object cell: `GcHeader` + `ObjectHeader` (class 0, the layout
/// word, no meta record) + one 8-byte slot per entry of `slots`. Returns the
/// slots `main` must tag ([`emit_tag_fixups`]).
pub(crate) fn emit_object_cell(
    llmod: &mut LlModule,
    name: &str,
    layout_word: u32,
    slots: &[Slot],
) -> Vec<TagFixup> {
    let triple = llmod.target_triple.clone();
    let header = crate::target_layout::object_header_size_bytes(&triple);
    let size = GC_HEADER_SIZE + header + 8 * slots.len() as u64;
    let ilp32 = crate::target_layout::target_is_ilp32(&triple);
    // ILP32 keeps the slot region 8-aligned with a pad word before `meta`.
    let (pad_ty, pad_init) = if ilp32 {
        ("i32, ", "i32 0, ")
    } else {
        ("", "")
    };
    let mut fixups = Vec::new();
    let mut types = Vec::new();
    let mut inits = Vec::new();
    for (index, slot) in slots.iter().enumerate() {
        let (ty, init, tag_at_startup) = slot_init(&triple, slot);
        types.push(ty);
        inits.push(format!("{ty} {init}"));
        if tag_at_startup {
            fixups.push(TagFixup {
                cell: name.to_string(),
                offset: GC_HEADER_SIZE + header + 8 * index as u64,
            });
        }
    }
    llmod.add_raw_global(format!(
        "@{name} = internal global {{ i8, i8, i16, i32, i32, i32, {pad_ty}ptr, {{ {} }} }} \
         {{ i8 {GC_TYPE_OBJECT}, i8 {IMAGE_CELL_FLAGS}, i16 0, i32 {size}, i32 0, i32 {}, {pad_init}ptr null, {{ {} }} {{ {} }} }}, \
         section \"{}\", align 8",
        types.join(", "),
        layout_word as i32,
        types.join(", "),
        inits.join(", "),
        section(&triple)
    ));
    fixups
}

/// OR the pointer tag into each listed slot. Runs in `main` before any module
/// initializer, on targets whose relocations cannot carry the tag.
pub(crate) fn emit_tag_fixups(blk: &mut LlBlock, fixups: &[TagFixup]) {
    for fixup in fixups {
        let offset = fixup.offset.to_string();
        let slot = blk.gep(I8, &format!("@{}", fixup.cell), &[(I64, offset.as_str())]);
        let raw = blk.load(I64, &slot);
        let tagged = blk.or(I64, &raw, crate::nanbox::POINTER_TAG_I64);
        blk.store(I64, &tagged, &slot);
    }
}

/// `PERRY_REALM_TEST_FIXTURE=1`: emit WRT step 1's fixture. A compile-time
/// diagnostic knob: off, nothing is emitted and nothing is linked.
pub(crate) fn test_fixture_requested() -> bool {
    std::env::var("PERRY_REALM_TEST_FIXTURE").as_deref() == Ok("1")
}

/// The fixture's cells, emitted before `main` exists (a module edit while a
/// function is open is not possible).
pub(crate) struct TestFixture {
    object: String,
    fixups: Vec<TagFixup>,
}

/// One function cell whose `info` is the runtime's `perry_realm_fixture_info`
/// (its body returns 42), and one `{value, done}` object on the runtime's
/// ITERATOR_RESULT layout whose `value` names the function cell and whose
/// `done` is `false`: the shape of a realm (prototype → method) built by the
/// path step 3 uses.
pub(crate) fn emit_test_fixture(llmod: &mut LlModule) -> TestFixture {
    let function = "perry_realm_fixture_fn";
    let object = "perry_realm_fixture_object";
    emit_closure_cell(
        llmod,
        function,
        "perry_realm_fixture_info",
        perry_abi::FUNCTION_LAYOUT_WORD,
    );
    let fixups = emit_object_cell(
        llmod,
        object,
        perry_abi::native_layout_word("ITERATOR_RESULT", 0),
        &[
            Slot::Cell(function.to_string()),
            Slot::Bits(crate::nanbox::TAG_FALSE),
        ],
    );
    llmod.declare_function("js_realm_test_fixture_publish", VOID, &[DOUBLE]);
    TestFixture {
        object: object.to_string(),
        fixups,
    }
}

/// In `main`, after the static shape seed: tag what the target could not, then
/// publish the object as `globalThis.__perryRealmFixture`.
pub(crate) fn emit_test_fixture_install(blk: &mut LlBlock, fixture: &TestFixture) {
    emit_tag_fixups(blk, &fixture.fixups);
    let header = GC_HEADER_SIZE.to_string();
    let user = blk.gep(
        I8,
        &format!("@{}", fixture.object),
        &[(I64, header.as_str())],
    );
    let address = blk.ptrtoint(&user, I64);
    let tagged = blk.or(I64, &address, crate::nanbox::POINTER_TAG_I64);
    let value = blk.bitcast_i64_to_double(&tagged);
    blk.call_void("js_realm_test_fixture_publish", &[(DOUBLE, &value)]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_ir(triple: &str) -> String {
        let mut module = LlModule::new(triple);
        let fixture = emit_test_fixture(&mut module);
        let block = module
            .define_function("main", VOID, vec![])
            .create_block("entry");
        emit_test_fixture_install(block, &fixture);
        block.ret_void();
        module.to_ir()
    }

    #[test]
    fn cells_land_in_the_realm_section_on_every_object_format() {
        for (triple, section) in [
            ("x86_64-unknown-linux-gnu", "perry_realm"),
            ("aarch64-apple-darwin", "__DATA,__perry_realm"),
            ("x86_64-pc-windows-msvc", ".perryrealm$m"),
            ("wasm32-wasip2", "perry_realm"),
        ] {
            let ir = fixture_ir(triple);
            assert_eq!(
                ir.matches(&format!("section \"{section}\", align 8"))
                    .count(),
                2,
                "{triple}: both cells in {section}\n{ir}"
            );
            assert!(
                ir.contains("@perry_realm_fixture_fn = internal global"),
                "{triple}"
            );
            assert!(ir.contains("ptr @perry_realm_fixture_info"), "{triple}");
            assert!(
                ir.contains("call void @js_realm_test_fixture_publish(double"),
                "{triple}"
            );
        }
    }

    #[test]
    fn a_cell_pointer_is_tagged_by_what_the_format_can_carry() {
        // ELF/COFF: one tagged link-time constant, no startup work.
        let linux = fixture_ir("x86_64-unknown-linux-gnu");
        assert!(linux.contains("add (i64 ptrtoint (ptr getelementptr (i8, ptr @perry_realm_fixture_fn, i64 8) to i64), i64 9222527611924643840)"));
        assert_eq!(linux.matches("load i64").count(), 0);
        // Apple: an untagged rebase, tagged in main.
        let mac = fixture_ir("aarch64-apple-darwin");
        assert!(!mac.contains("add (i64 ptrtoint"));
        assert!(mac.contains(
            "i64 ptrtoint (ptr getelementptr (i8, ptr @perry_realm_fixture_fn, i64 8) to i64)"
        ));
        assert_eq!(mac.matches("load i64").count(), 1, "{mac}");
        // wasm32: address half relocated, tag half constant.
        let wasm = fixture_ir("wasm32-wasip2");
        assert!(wasm.contains("{ i32 ptrtoint (ptr getelementptr (i8, ptr @perry_realm_fixture_fn, i64 8) to i32), i32 2147287040 }"), "{wasm}");
        assert_eq!(wasm.matches("load i64").count(), 0);
    }

    #[test]
    fn the_cell_sizes_are_the_runtime_layouts() {
        let linux = fixture_ir("x86_64-unknown-linux-gnu");
        // GcHeader 8 + ClosureHeader 24; GcHeader 8 + ObjectHeader 16 + 2 slots.
        assert!(linux.contains("{ i8 4, i8 5, i16 0, i32 32,"));
        assert!(linux.contains("{ i8 2, i8 5, i16 0, i32 40,"));
        let wasm = fixture_ir("wasm32-wasip2");
        assert!(wasm.contains("{ i8 4, i8 5, i16 0, i32 24,"), "{wasm}");
        assert!(wasm.contains("{ i8 2, i8 5, i16 0, i32 40,"), "{wasm}");
    }
}
