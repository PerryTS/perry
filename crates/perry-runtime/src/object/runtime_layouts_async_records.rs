static IMMEDIATE_KEYS: [PerryLayoutKey; 0] = [];
static IMMEDIATE_INDEX: [u32; 2] = [u32::MAX, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut IMMEDIATE: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540008,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: &raw const IMMEDIATE_PROTO_FAMILY,
    keys: IMMEDIATE_KEYS.as_ptr(),
    key_count: 0,
    index_mask: 1,
    index: IMMEDIATE_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(IMMEDIATE);

pub(crate) const URL_CLASS_ID: u32 = 0xFFFF0063;

#[path = "runtime_layouts/stream_prototypes.rs"]
mod stream_prototypes;
pub(crate) use stream_prototypes::*;
#[path = "runtime_layouts/stream_instances.rs"]
mod stream_instances;
pub(crate) use stream_instances::*;
#[path = "runtime_layouts/http_instances.rs"]
mod http_instances;
pub(crate) use http_instances::*;
#[used(linker)]
#[export_name = "perry_image_literal_0_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_27: ImageString<6> = image(*b"index\0");
wasm_string_image!(IMAGE_27);
#[used(linker)]
#[export_name = "perry_atom_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_27: StaticAtom = StaticAtom {
    hash: atom_hash(b"index"),
    string: std::ptr::addr_of!(IMAGE_27.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_27);
#[used(linker)]
#[export_name = "perry_image_literal_0_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_28: ImageString<6> = image(*b"input\0");
wasm_string_image!(IMAGE_28);
#[used(linker)]
#[export_name = "perry_atom_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_28: StaticAtom = StaticAtom {
    hash: atom_hash(b"input"),
    string: std::ptr::addr_of!(IMAGE_28.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_28);
#[used(linker)]
#[export_name = "perry_image_literal_0_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_29: ImageString<7> = image(*b"groups\0");
wasm_string_image!(IMAGE_29);
#[used(linker)]
#[export_name = "perry_atom_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_29: StaticAtom = StaticAtom {
    hash: atom_hash(b"groups"),
    string: std::ptr::addr_of!(IMAGE_29.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_29);
#[used(linker)]
#[export_name = "perry_image_literal_0_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static IMAGE_30: ImageString<8> = image(*b"indices\0");
wasm_string_image!(IMAGE_30);
#[used(linker)]
#[export_name = "perry_atom_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
static ATOM_30: StaticAtom = StaticAtom {
    hash: atom_hash(b"indices"),
    string: std::ptr::addr_of!(IMAGE_30.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(ATOM_30);
/// `Symbol.iterator` as a layout key (§6a, a well-known symbol atom).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_well_known_iterator"]
static ATOM_SYMBOL_ITERATOR: StaticAtom = StaticAtom {
    hash: crate::codegen_abi::well_known_symbol_atom_hash(
        crate::codegen_abi::WELL_KNOWN_SYMBOL_ITERATOR,
    ),
    string: std::ptr::null(),
    kind: crate::codegen_abi::ATOM_KIND_WELL_KNOWN_SYMBOL,
    _pad: [0; 3],
    index: crate::codegen_abi::WELL_KNOWN_SYMBOL_ITERATOR,
};
wasm_atom!(ATOM_SYMBOL_ITERATOR);
static REGEX_EXEC_NAMED_KEYS: [PerryLayoutKey; 4] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_27),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_28),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_29),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
];
static REGEX_EXEC_NAMED_INDEX: [u32; 8] = [u32::MAX, u32::MAX, u32::MAX, 1, 2, 0, 3, u32::MAX];
/// A `RegExp.prototype.exec` result array's named layout (`ArrayHeader.named`):
/// `index`, `input`, `groups`.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut REGEX_EXEC_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540009,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 4,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: REGEX_EXEC_NAMED_KEYS.as_ptr(),
    key_count: 4,
    index_mask: 7,
    index: REGEX_EXEC_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(REGEX_EXEC_NAMED);
static REGEX_EXEC_INDICES_NAMED_KEYS: [PerryLayoutKey; 5] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_27),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_28),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_29),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_30),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
];
static REGEX_EXEC_INDICES_NAMED_INDEX: [u32; 16] = [
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    u32::MAX,
    0,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    1,
    2,
    3,
    u32::MAX,
    u32::MAX,
];
/// A `d`-flag exec result's named layout: `index`, `input`, `groups`, `indices`.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut REGEX_EXEC_INDICES_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x5254000A,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 5,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: REGEX_EXEC_INDICES_NAMED_KEYS.as_ptr(),
    key_count: 5,
    index_mask: 15,
    index: REGEX_EXEC_INDICES_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(REGEX_EXEC_INDICES_NAMED);
static REGEX_INDICES_GROUPS_NAMED_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_29),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static REGEX_INDICES_GROUPS_NAMED_INDEX: [u32; 4] = [u32::MAX, 0, 1, u32::MAX];
/// A `d`-flag `indices` array's named layout: `groups`.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut REGEX_INDICES_GROUPS_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x5254000B,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: REGEX_INDICES_GROUPS_NAMED_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: REGEX_INDICES_GROUPS_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(REGEX_INDICES_GROUPS_NAMED);
static ARRAY_NAMED_KEYS: [PerryLayoutKey; 1] = [PerryLayoutKey {
    atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
    slot: 0,
    attrs: 0,
    _pad: [0; 3],
}];
static ARRAY_NAMED_INDEX: [u32; 2] = [u32::MAX, 0];
/// Empty array named family; absent iterator slot 0 is the hole marker.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ARRAY_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x5254000D,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 1,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: ARRAY_NAMED_KEYS.as_ptr(),
    key_count: 1,
    index_mask: 1,
    index: ARRAY_NAMED_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(ARRAY_NAMED);

// Fixed legacy SQLite column metadata; the five data properties are all present.
#[used(linker)]
#[export_name = "perry_image_literal_0_636f6c756d6e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_0: ImageString<7> = image(*b"column\0");
wasm_string_image!(SQLITE_IMAGE_0);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_636f6c756d6e"]
static SQLITE_ATOM_0: StaticAtom = StaticAtom {
    hash: atom_hash(b"column"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_0.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_0);
#[used(linker)]
#[export_name = "perry_image_literal_0_6461746162617365"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_1: ImageString<9> = image(*b"database\0");
wasm_string_image!(SQLITE_IMAGE_1);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_6461746162617365"]
static SQLITE_ATOM_1: StaticAtom = StaticAtom {
    hash: atom_hash(b"database"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_1.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_1);
#[used(linker)]
#[export_name = "perry_image_literal_0_6e616d65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_2: ImageString<5> = image(*b"name\0");
wasm_string_image!(SQLITE_IMAGE_2);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_6e616d65"]
static SQLITE_ATOM_2: StaticAtom = StaticAtom {
    hash: atom_hash(b"name"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_2.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_2);
#[used(linker)]
#[export_name = "perry_image_literal_0_7461626c65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_3: ImageString<6> = image(*b"table\0");
wasm_string_image!(SQLITE_IMAGE_3);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_7461626c65"]
static SQLITE_ATOM_3: StaticAtom = StaticAtom {
    hash: atom_hash(b"table"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_3.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_3);
#[used(linker)]
#[export_name = "perry_image_literal_0_74797065"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_4: ImageString<5> = image(*b"type\0");
wasm_string_image!(SQLITE_IMAGE_4);
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
#[export_name = "perry_atom_74797065"]
static SQLITE_ATOM_4: StaticAtom = StaticAtom {
    hash: atom_hash(b"type"),
    string: std::ptr::addr_of!(SQLITE_IMAGE_4.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
wasm_atom!(SQLITE_ATOM_4);
static SQLITE_COLUMN_KEYS: [PerryLayoutKey; 5] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_0),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_1),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_2),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_3),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(SQLITE_ATOM_4),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
];
static SQLITE_COLUMN_INDEX: [u32; 16] = [
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    2,
    0,
    1,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    3,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_layouts"
)]
pub static mut SQLITE_COLUMN_METADATA: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53514c01,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 5,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: SQLITE_COLUMN_KEYS.as_ptr(),
    key_count: 5,
    index_mask: 15,
    index: SQLITE_COLUMN_INDEX.as_ptr(),
    class_id: 0,
    parent_class_id: 0,
    methods: std::ptr::null(),
};
wasm_layout!(SQLITE_COLUMN_METADATA);
