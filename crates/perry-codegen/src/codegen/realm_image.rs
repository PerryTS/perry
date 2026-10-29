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
// The composed cell emitters below are exercised by their tests only; the
// production entry that hands them the realm plan is the next phase (see the
// WRT3b PR body). Remove this allow when that entry lands.
#![allow(dead_code)]

use crate::block::LlBlock;
use crate::module::LlModule;
use crate::types::{DOUBLE, I64, I8, VOID};

pub(crate) mod arrays;
pub(crate) mod atoms;
pub(crate) mod functions;
pub(crate) mod header;
#[cfg(test)]
mod key_list_tests;
pub(crate) mod layouts;
pub(crate) mod objects;
pub(crate) mod properties;
mod records;
pub(crate) mod sets;
pub(crate) use records::{
    emit_accessor_pair, emit_metadata_cell, emit_metadata_with_expando, emit_symbol_cell,
};

const GC_TYPE_ARRAY: u8 = 1;
const GC_TYPE_OBJECT: u8 = crate::runtime_abi::GC_TYPE_OBJECT;
const GC_TYPE_CLOSURE: u8 = 4;
/// `GC_FLAG_MARKED | GC_FLAG_PINNED`: an image cell is never white and never
/// moves (perry-runtime `realm::IMAGE_CELL_FLAGS`).
const IMAGE_CELL_FLAGS: u8 = 0x01 | 0x04;
/// A canonical key list also carries `GC_FLAG_SHAPE_SHARED`
/// (`js_array_alloc_canonical_keys`).
const KEY_LIST_FLAGS: u8 = IMAGE_CELL_FLAGS | 0x08;
const GC_HEADER_SIZE: u64 = crate::target_layout::GC_HEADER_SIZE_BYTES;
/// `ArrayHeader`: length, capacity, `named`, element kind prefix and kind;
/// 24 bytes on LP64 and (with its pad word) on ILP32.
const ARRAY_HEADER_SIZE: u64 = 24;
const POINTER_TAG: u64 = crate::nanbox::POINTER_TAG;
const STRING_TAG: u64 = crate::nanbox::STRING_TAG;

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
    /// A user address of this cell, with its value tag (zero for a raw
    /// capture, POINTER_TAG for a JavaScript reference).
    Cell(String, u64),
    /// A NaN-boxed string pointer to the image string whose record has this
    /// symbol (`perry_image_literal_*`): a key of a canonical key list.
    ImageString(String),
    /// An address within static storage, including untagged byte captures.
    /// Offsets are relative to the named record, not its GC user address.
    Address {
        symbol: String,
        offset: u64,
        tag: u64,
    },
}

/// A slot `main` must tag before the program runs (64-bit Apple targets).
#[derive(Clone, Debug)]
pub(crate) struct TagFixup {
    pub(crate) cell: String,
    pub(crate) offset: u64,
    pub(crate) tag: u64,
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

fn llvm_identifier(name: &str) -> String {
    if !name.is_empty()
        && !name.as_bytes()[0].is_ascii_digit()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.$-".contains(&byte))
    {
        return name.into();
    }
    let mut quoted = String::from("\"");
    for byte in name.bytes() {
        if (0x20..0x7f).contains(&byte) && byte != b'"' && byte != b'\\' {
            quoted.push(char::from(byte));
        } else {
            quoted.push_str(&format!("\\{byte:02X}"));
        }
    }
    quoted.push('"');
    quoted
}

fn user_address(cell: &str) -> String {
    format!("getelementptr (i8, ptr @{cell}, i64 {GC_HEADER_SIZE})")
}

/// One 8-byte slot as `(llvm type, initializer)`, plus the tag startup code
/// must OR in when the format cannot carry it.
fn slot_init(triple: &str, slot: &Slot) -> (&'static str, String, Option<u64>) {
    let mode = pointer_slots(triple);
    let (address, tag) = match slot {
        Slot::Bits(bits) => {
            return match mode {
                PointerSlots::SplitHalves => (
                    "{ i32, i32 }",
                    format!(
                        "{{ i32 {}, i32 {} }}",
                        *bits as u32 as i32,
                        (*bits >> 32) as u32 as i32
                    ),
                    None,
                ),
                _ => ("i64", crate::nanbox::i64_literal(*bits), None),
            };
        }
        Slot::Cell(cell, tag) => (user_address(cell), *tag),
        Slot::ImageString(record) => (user_address(record), STRING_TAG),
        Slot::Address {
            symbol,
            offset,
            tag,
        } => (
            format!(
                "getelementptr (i8, ptr @{}, i64 {offset})",
                llvm_identifier(symbol)
            ),
            *tag,
        ),
    };
    match mode {
        PointerSlots::SplitHalves => (
            "{ i32, i32 }",
            format!(
                "{{ i32 ptrtoint (ptr {address} to i32), i32 {} }}",
                (tag >> 32) as u32 as i32
            ),
            None,
        ),
        PointerSlots::TaggedRelocation => (
            "i64",
            format!(
                "add (i64 ptrtoint (ptr {address} to i64), i64 {})",
                crate::nanbox::i64_literal(tag)
            ),
            None,
        ),
        PointerSlots::TaggedAtStartup => (
            "i64",
            format!("ptrtoint (ptr {address} to i64)"),
            (tag != 0).then_some(tag),
        ),
    }
}

/// All mutable fields of a function cell are born with the function. Its
/// property bag and captures are ordinary traced edges, including back-edges.
/// `capture_flags` excludes the count, which is derived from `captures`.
pub(crate) struct ClosureCell<'a> {
    pub(crate) exported: bool,
    pub(crate) info_symbol: &'a str,
    pub(crate) shape: u32,
    pub(crate) reserved: u16,
    pub(crate) capture_flags: u32,
    pub(crate) props: Option<&'a str>,
    pub(crate) captures: &'a [Slot],
}

/// A function object cell with the exact `ClosureHeader` payload and trailing
/// capture storage. `info` names the body's exported static `JsFunctionInfo`;
/// the properties pointer is raw, while captures follow their declared ABI.
pub(crate) fn emit_closure_cell_with_fields(
    llmod: &mut LlModule,
    name: &str,
    cell: &ClosureCell<'_>,
) -> Vec<TagFixup> {
    let triple = llmod.target_triple.clone();
    let linkage = if cell.exported { "" } else { "internal " };
    // Rust's source-derived export spelling can contain qualified paths and
    // ABI builder punctuation. LLVM identifiers quote those bytes verbatim.
    let info_symbol = llvm_identifier(cell.info_symbol);
    let declaration = format!("@{info_symbol} = external global i8");
    if !llmod.diagnostic_globals().any(|line| line == declaration) {
        llmod.add_raw_global(declaration);
    }
    let capture_count = u32::try_from(cell.captures.len()).expect("realm capture count fits u32");
    const CAPTURE_COUNT_MASK: u32 = (1 << 23) - 1;
    assert_eq!(cell.capture_flags & CAPTURE_COUNT_MASK, 0);
    assert!(capture_count <= CAPTURE_COUNT_MASK);
    let count_and_flags = cell.capture_flags | capture_count;
    let header_size = GC_HEADER_SIZE + crate::target_layout::closure_header_size_bytes(&triple);
    let size = header_size + 8 * u64::from(capture_count);
    let props = cell.props.map_or_else(|| "null".into(), user_address);
    let mut fixups = Vec::new();
    let mut types = Vec::new();
    let mut inits = Vec::new();
    for (index, slot) in cell.captures.iter().enumerate() {
        let (ty, init, tag) = slot_init(&triple, slot);
        types.push(ty);
        inits.push(format!("{ty} {init}"));
        if let Some(tag) = tag {
            fixups.push(TagFixup {
                cell: name.to_string(),
                offset: header_size + 8 * index as u64,
                tag,
            });
        }
    }
    llmod.add_raw_global(format!(
        "@{name} = {linkage}global {{ i8, i8, i16, i32, i32, i32, ptr, ptr, {{ {} }} }} \
         {{ i8 {GC_TYPE_CLOSURE}, i8 {IMAGE_CELL_FLAGS}, i16 {}, i32 {size}, i32 {}, i32 {}, ptr @{info_symbol}, ptr {props}, {{ {} }} {{ {} }} }}, \
         section \"{}\", align 8",
        types.join(", "),
        cell.reserved,
        count_and_flags as i32,
        cell.shape as i32,
        types.join(", "),
        inits.join(", "),
        section(&triple)
    ));
    fixups
}

/// Empty function cell used by the image/collector fixture.
pub(crate) fn emit_closure_cell(llmod: &mut LlModule, name: &str, info_symbol: &str, shape: u32) {
    emit_closure_cell_with_fields(
        llmod,
        name,
        &ClosureCell {
            exported: false,
            info_symbol,
            shape,
            reserved: perry_abi::GC_LAYOUT_POINTER_FREE,
            capture_flags: 0,
            props: None,
            captures: &[],
        },
    );
}

/// An object owns its metadata and inline slots. Prototype and constructor
/// identities are birth facts, not values to reconstruct in an installer.
pub(crate) struct ObjectCell<'a> {
    pub(crate) exported: bool,
    pub(crate) class_id: u32,
    pub(crate) layout_word: u32,
    pub(crate) reserved: u16,
    pub(crate) meta: Option<&'a str>,
    pub(crate) slots: &'a [Slot],
}

pub(crate) fn emit_object_cell(
    llmod: &mut LlModule,
    name: &str,
    layout_word: u32,
    reserved: u16,
    slots: &[Slot],
) -> Vec<TagFixup> {
    emit_object_cell_with_fields(
        llmod,
        name,
        &ObjectCell {
            exported: false,
            class_id: 0,
            layout_word,
            reserved,
            meta: None,
            slots,
        },
    )
}

/// `GcHeader` + `ObjectHeader` + inline slots, including an optional raw
/// metadata edge. Only tagged value slots participate in Apple's tag pass.
pub(crate) fn emit_object_cell_with_fields(
    llmod: &mut LlModule,
    name: &str,
    cell: &ObjectCell<'_>,
) -> Vec<TagFixup> {
    let linkage = if cell.exported { "" } else { "internal " };
    let class_id = cell.class_id as i32;
    let layout_word = cell.layout_word;
    let reserved = cell.reserved;
    let slots = cell.slots;
    // The image cell's metadata edge: this record's own field, not a read of a
    // live ObjectHeader.
    let ObjectCell { meta, .. } = cell;
    let meta = meta.map_or_else(|| "null".into(), user_address);
    let triple = llmod.target_triple.clone();
    let header = crate::target_layout::object_slot_base_bytes(
        &triple,
        crate::target_layout::HeaderForm::Meta,
    );
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
        if let Some(tag) = tag_at_startup {
            fixups.push(TagFixup {
                cell: name.to_string(),
                offset: GC_HEADER_SIZE + header + 8 * index as u64,
                tag,
            });
        }
    }
    llmod.add_raw_global(format!(
        "@{name} = {linkage}global {{ i8, i8, i16, i32, i32, i32, {pad_ty}ptr, {{ {} }} }} \
         {{ i8 {GC_TYPE_OBJECT}, i8 {IMAGE_CELL_FLAGS}, i16 {reserved}, i32 {size}, i32 {class_id}, i32 {}, {pad_init}ptr {meta}, {{ {} }} {{ {} }} }}, \
         section \"{}\", align 8",
        types.join(", "),
        layout_word as i32,
        types.join(", "),
        inits.join(", "),
        section(&triple)
    ));
    fixups
}

/// A canonical key list cell (WRT step 2): `GcHeader` + `ArrayHeader` + one
/// slot per key, laid out as `js_array_alloc_canonical_keys` lays out a heap
/// list with no attributes: length and capacity are the key count, `named`
/// is null, the element kind words are zero. Each slot is the NaN-boxed
/// pointer of a key's image string record (`records`, in key order).
/// Returns the slots startup code must tag ([`emit_tag_fixups`]).
pub(crate) fn emit_key_list_cell(
    llmod: &mut LlModule,
    name: &str,
    records: &[String],
) -> Vec<TagFixup> {
    emit_array_cell(
        llmod,
        name,
        &records
            .iter()
            .cloned()
            .map(Slot::ImageString)
            .collect::<Vec<_>>(),
        records.len(),
        perry_abi::GC_LAYOUT_SIDE_MASK | perry_abi::GC_LAYOUT_ALL_POINTERS,
    )
}

/// A descriptor's keys, attributes and storage permutation as image cells.
/// Process-lifetime symbols are resolved before publication; per-evaluation
/// symbol bindings deliberately keep their dynamic path.
pub(crate) fn emit_layout_key_list(
    llmod: &mut LlModule,
    name: &str,
    keys: &[(String, u64, u8, u32)],
) -> String {
    if keys.is_empty() {
        return "null".into();
    }
    let mut slots = Vec::new();
    for (atom, _, _, _) in keys {
        if atom.starts_with("perry_atom_sym_") {
            slots.push(Slot::Bits(crate::nanbox::TAG_UNDEFINED));
            continue;
        }
        let Some(hex) = atom.strip_prefix("perry_atom_") else {
            return "null".into();
        };
        if hex.is_empty() || hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return "null".into();
        }
        slots.push(Slot::ImageString(format!("perry_image_literal_0_{hex}")));
    }
    let entries = keys
        .iter()
        .zip(slots)
        .map(|((atom, hash, attrs, slot), value)| ImageKeySlot {
            value,
            string_hash: (!atom.starts_with("perry_atom_sym_")).then_some(*hash),
            attrs: *attrs,
            slot: *slot,
        })
        .collect::<Vec<_>>();
    // Existing descriptor births perform their target's fixups before linking
    // the list. Realm emission consumes the returned fixups for its tag walk.
    emit_image_key_list(llmod, name, &entries).0
}

/// One canonical key, with its actual image value. A symbol key names its
/// symbol cell directly; its description's hash is never a string-key bloom
/// bit. Attributes and storage positions use the ordinary key-list ABI.
pub(crate) struct ImageKeySlot {
    pub(crate) value: Slot,
    pub(crate) string_hash: Option<u64>,
    pub(crate) attrs: u8,
    pub(crate) slot: u32,
}

/// The common image-list emitter for program, runtime and realm layouts.
/// The caller supplies already-resolved keys, so a realm symbol never needs
/// a runtime population store. Returns the user pointer and Apple tag slots.
pub(crate) fn emit_image_key_list(
    llmod: &mut LlModule,
    name: &str,
    keys: &[ImageKeySlot],
) -> (String, Vec<TagFixup>) {
    if keys.is_empty() {
        return ("null".into(), Vec::new());
    }
    let mapped = keys
        .iter()
        .enumerate()
        .any(|(i, key)| key.slot as usize != i);
    let attributes = mapped || keys.iter().any(|key| key.attrs != 0);
    let mut slots = keys.iter().map(|key| key.value.clone()).collect::<Vec<_>>();
    let mut reserved = perry_abi::GC_LAYOUT_SIDE_MASK | perry_abi::GC_LAYOUT_ALL_POINTERS;
    let mut fixups = Vec::new();
    if attributes {
        let attrs_name = format!("{name}_attrs");
        let mut previous = 0;
        let mut words = Vec::new();
        for key in keys {
            previous = perry_abi::canonical_key_attribute_word(
                previous,
                key.attrs,
                key.string_hash.map_or(0, |hash| 1u16 << (hash >> 60)),
                mapped.then_some(key.slot),
            );
            words.push(Slot::Bits(previous));
        }
        fixups.extend(emit_array_cell(
            llmod,
            &attrs_name,
            &words,
            words.len(),
            perry_abi::GC_LAYOUT_POINTER_FREE,
        ));
        slots.insert(0, Slot::Cell(attrs_name, POINTER_TAG));
        reserved |= perry_abi::GC_ARRAY_KEY_ATTRS;
    }
    fixups.extend(emit_array_cell(llmod, name, &slots, keys.len(), reserved));
    (user_address(name), fixups)
}

/// Arrays used as realm content must evacuate with the realm. Only canonical
/// key lists (and their attribute storage) carry SHAPE_SHARED. A named-property
/// holder is a raw edge, independent of the indexed element slots.
pub(crate) struct ArrayCell<'a> {
    pub(crate) exported: bool,
    pub(crate) shared: bool,
    pub(crate) reserved: u16,
    pub(crate) length: u32,
    pub(crate) capacity: u32,
    pub(crate) dense_prefix: u32,
    pub(crate) named: Option<&'a str>,
    pub(crate) slots: &'a [Slot],
}

fn emit_array_cell(
    llmod: &mut LlModule,
    name: &str,
    slots: &[Slot],
    count: usize,
    reserved: u16,
) -> Vec<TagFixup> {
    let count = u32::try_from(count).expect("realm array length fits u32");
    emit_array_cell_with_fields(
        llmod,
        name,
        &ArrayCell {
            exported: false,
            shared: true,
            reserved,
            length: count,
            capacity: count,
            dense_prefix: 0,
            named: None,
            slots,
        },
    )
}

pub(crate) fn emit_array_cell_with_fields(
    llmod: &mut LlModule,
    name: &str,
    cell: &ArrayCell<'_>,
) -> Vec<TagFixup> {
    let linkage = if cell.exported { "" } else { "internal " };
    let flags = if cell.shared {
        KEY_LIST_FLAGS
    } else {
        IMAGE_CELL_FLAGS
    };
    let reserved = cell.reserved;
    let length = cell.length;
    let capacity = cell.capacity;
    let dense_prefix = cell.dense_prefix;
    assert!(dense_prefix <= length);
    let named = cell.named.map_or_else(|| "null".into(), user_address);
    let slots = cell.slots;
    assert!(length <= capacity);
    // Attribute-bearing key lists have one front-reserve pointer word.
    let front = usize::from(reserved & perry_abi::GC_ARRAY_KEY_ATTRS != 0);
    assert_eq!(slots.len(), capacity as usize + front);
    assert!(!cell.shared || cell.named.is_none());
    let triple = llmod.target_triple.clone();
    let size = GC_HEADER_SIZE + ARRAY_HEADER_SIZE + 8 * slots.len() as u64;
    let (pad_ty, pad_init) = if crate::target_layout::target_is_ilp32(&triple) {
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
        if let Some(tag) = tag_at_startup {
            fixups.push(TagFixup {
                cell: name.to_string(),
                offset: GC_HEADER_SIZE + ARRAY_HEADER_SIZE + 8 * index as u64,
                tag,
            });
        }
    }
    llmod.add_raw_global(format!(
        "@{name} = {linkage}global {{ i8, i8, i16, i32, i32, i32, ptr, {pad_ty}i32, i32, {{ {} }} }} \
         {{ i8 {GC_TYPE_ARRAY}, i8 {flags}, i16 {reserved}, i32 {size}, i32 {length}, i32 {capacity}, ptr {named}, {pad_init}i32 {dense_prefix}, i32 0, {{ {} }} {{ {} }} }}, \
         section \"{}\", align 8",
        types.join(", "), types.join(", "), inits.join(", "), section(&triple)
    ));
    fixups
}

/// OR each listed slot's tag in. Runs before any module initializer (in
/// `main`, or a constructor of the unit that owns the cells), on targets
/// whose relocations cannot carry the tag.
pub(crate) fn emit_tag_fixups(blk: &mut LlBlock, fixups: &[TagFixup]) {
    for fixup in fixups {
        let offset = fixup.offset.to_string();
        let slot = blk.gep(I8, &format!("@{}", fixup.cell), &[(I64, offset.as_str())]);
        let raw = blk.load(I64, &slot);
        let tagged = blk.or(I64, &raw, &crate::nanbox::i64_literal(fixup.tag));
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
        perry_abi::OBJ_FLAG_PLAIN_ORDINARY,
        &[
            Slot::Cell(function.to_string(), POINTER_TAG),
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
    fn captured_functions_keep_props_flags_and_relocations_on_every_target() {
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(triple);
            emit_object_cell(&mut module, "props", 0x8000_0000, 0x0200, &[]);
            let captures = [
                Slot::Cell("props".into(), POINTER_TAG),
                Slot::Cell("props".into(), 0),
                Slot::Bits(0x3ff0_0000_0000_0000),
            ];
            let fields = ClosureCell {
                exported: true,
                info_symbol: "body_info::with_rest(0)",
                shape: perry_abi::FUNCTION_LAYOUT_WORD,
                reserved: 0,
                capture_flags: 1 << 23,
                props: Some("props"),
                captures: &captures,
            };
            let fixups = emit_closure_cell_with_fields(&mut module, "function", &fields);
            // Several functions can share their body and info; LLVM permits
            // precisely one declaration of that symbol in this module.
            emit_closure_cell_with_fields(&mut module, "other_function", &fields);
            let ir = module.to_ir();
            assert_eq!(
                ir.matches("@\"body_info::with_rest(0)\" = external global")
                    .count(),
                1
            );
            let header = GC_HEADER_SIZE + crate::target_layout::closure_header_size_bytes(triple);
            assert!(
                ir.contains(&format!(
                    "i8 4, i8 5, i16 0, i32 {}, i32 8388611,",
                    header + 24
                )),
                "{triple}: {ir}"
            );
            assert!(ir.contains(
                "ptr @\"body_info::with_rest(0)\", ptr getelementptr (i8, ptr @props, i64 8)"
            ));
            if triple.contains("apple") {
                assert_eq!(fixups.len(), 1, "raw captures and props must stay raw");
                assert_eq!(fixups[0].offset, header);
                assert_eq!(fixups[0].tag, POINTER_TAG);
            } else {
                assert!(fixups.is_empty());
            }
            let context = inkwell::context::Context::create();
            let parsed = crate::inprocess::parse_ir_text(&context, &ir, "captured_realm")
                .expect("captured realm cells parse on every ABI");
            parsed.verify().expect("captured realm IR verifies");
        }
    }

    #[test]
    fn realm_arrays_and_accessor_pairs_are_not_shared_key_lists() {
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "x86_64-pc-windows-msvc",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(triple);
            emit_closure_cell(
                &mut module,
                "getter",
                "getter_info",
                perry_abi::FUNCTION_LAYOUT_WORD,
            );
            let pair = [
                Slot::Cell("getter".into(), POINTER_TAG),
                Slot::Bits(crate::nanbox::TAG_UNDEFINED),
                Slot::Bits(0),
                Slot::Bits(0),
            ];
            emit_array_cell_with_fields(
                &mut module,
                "accessor",
                &ArrayCell {
                    exported: false,
                    shared: false,
                    reserved: 0,
                    length: 4,
                    capacity: 4,
                    dense_prefix: 0,
                    named: None,
                    slots: &pair,
                },
            );
            emit_object_cell_with_fields(
                &mut module,
                "named",
                &ObjectCell {
                    exported: false,
                    class_id: 42,
                    layout_word: 0x8000_0042,
                    reserved: perry_abi::OBJ_FLAG_PLAIN_ORDINARY,
                    meta: None,
                    slots: &[Slot::Cell("accessor".into(), POINTER_TAG)],
                },
            );
            let fixups = emit_array_cell_with_fields(
                &mut module,
                "perry_realm_Array_prototype",
                &ArrayCell {
                    exported: true,
                    shared: false,
                    reserved: perry_abi::GC_LAYOUT_POINTER_FREE,
                    length: 0,
                    capacity: 0,
                    dense_prefix: 0,
                    named: Some("named"),
                    slots: &[],
                },
            );
            assert!(fixups.is_empty(), "a raw named edge is never tagged");
            let ir = module.to_ir();
            assert!(ir.contains("@perry_realm_Array_prototype = global"));
            assert!(ir.contains("i8 1, i8 5, i16 0, i32 64, i32 4, i32 4"));
            assert!(ir.contains("i8 1, i8 5, i16 16384, i32 32, i32 0, i32 0, ptr getelementptr (i8, ptr @named, i64 8)"));
            assert!(ir.contains("i32 42, i32 -2147483582"));
            assert!(
                !ir.contains("i8 1, i8 13"),
                "only canonical lists may be SHAPE_SHARED"
            );
            let context = inkwell::context::Context::create();
            let parsed = crate::inprocess::parse_ir_text(&context, &ir, "realm_arrays").unwrap();
            parsed.verify().unwrap();
        }
    }

    #[test]
    fn process_symbols_have_image_slots_but_binding_symbols_do_not() {
        for triple in [
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "wasm32-wasip2",
        ] {
            let mut module = LlModule::new(triple);
            let keys = [(
                "perry_atom_sym_014040776b3a30".into(),
                0xffff_ffff_ffff_ffff,
                1,
                0,
            )];
            assert_ne!(
                emit_layout_key_list(&mut module, "symbol_keys", &keys),
                "null"
            );
            let ir = module.to_ir();
            let literal = |bits: u64| {
                if crate::target_layout::target_is_ilp32(triple) {
                    format!(
                        "i32 {}, i32 {}",
                        bits as u32 as i32,
                        (bits >> 32) as u32 as i32
                    )
                } else {
                    format!("i64 {}", bits as i64)
                }
            };
            assert!(
                ir.contains(&literal(crate::nanbox::TAG_UNDEFINED)),
                "{triple}: {ir}"
            );
            let expected = perry_abi::canonical_key_attribute_word(0, 1, 0, None);
            assert!(
                ir.contains(&literal(expected)),
                "symbol attributes carry no string Bloom bit: {ir}"
            );
            let context = inkwell::context::Context::create();
            crate::inprocess::parse_ir_text(&context, &ir, "symbol_keys")
                .expect("symbol-list image IR parses");
            let binding = [("perry_atom_local_module_7".into(), 0, 0, 0)];
            assert_eq!(
                emit_layout_key_list(&mut module, "bound_keys", &binding),
                "null"
            );
        }
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
        assert!(linux.contains("{ i8 4, i8 5, i16 16384, i32 32,"));
        assert!(linux.contains("{ i8 2, i8 5, i16 512, i32 40,"));
        let wasm = fixture_ir("wasm32-wasip2");
        assert!(wasm.contains("{ i8 4, i8 5, i16 16384, i32 24,"), "{wasm}");
        assert!(wasm.contains("{ i8 2, i8 5, i16 512, i32 40,"), "{wasm}");
    }
}
