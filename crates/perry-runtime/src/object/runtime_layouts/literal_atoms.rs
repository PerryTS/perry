use super::*;

#[used(linker)]
#[export_name = "perry_image_literal_0_6869676857617465724d61726b"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_0: ImageString<14> = image(*b"highWaterMark\0");

wasm_string_image!(IMAGE_0);

#[used(linker)]
#[export_name = "perry_atom_6869676857617465724d61726b"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_0: StaticAtom = StaticAtom {
    hash: atom_hash(b"highWaterMark"),
    string: std::ptr::addr_of!(IMAGE_0.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_0);

#[used(linker)]
#[export_name = "perry_image_literal_0_627566666572"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_1: ImageString<7> = image(*b"buffer\0");

wasm_string_image!(IMAGE_1);

#[used(linker)]
#[export_name = "perry_atom_627566666572"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_1: StaticAtom = StaticAtom {
    hash: atom_hash(b"buffer"),
    string: std::ptr::addr_of!(IMAGE_1.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_1);

#[used(linker)]
#[export_name = "perry_image_literal_0_627566666572496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_2: ImageString<12> = image(*b"bufferIndex\0");

wasm_string_image!(IMAGE_2);

#[used(linker)]
#[export_name = "perry_atom_627566666572496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_2: StaticAtom = StaticAtom {
    hash: atom_hash(b"bufferIndex"),
    string: std::ptr::addr_of!(IMAGE_2.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_2);

#[used(linker)]
#[export_name = "perry_image_literal_0_6c656e677468"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_3: ImageString<7> = image(*b"length\0");

wasm_string_image!(IMAGE_3);

#[used(linker)]
#[export_name = "perry_atom_6c656e677468"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_3: StaticAtom = StaticAtom {
    hash: atom_hash(b"length"),
    string: std::ptr::addr_of!(IMAGE_3.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_3);

#[used(linker)]
#[export_name = "perry_image_literal_0_7069706573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_4: ImageString<6> = image(*b"pipes\0");

wasm_string_image!(IMAGE_4);

#[used(linker)]
#[export_name = "perry_atom_7069706573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_4: StaticAtom = StaticAtom {
    hash: atom_hash(b"pipes"),
    string: std::ptr::addr_of!(IMAGE_4.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_4);

#[used(linker)]
#[export_name = "perry_image_literal_0_6177616974447261696e57726974657273"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_5: ImageString<18> = image(*b"awaitDrainWriters\0");

wasm_string_image!(IMAGE_5);

#[used(linker)]
#[export_name = "perry_atom_6177616974447261696e57726974657273"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_5: StaticAtom = StaticAtom {
    hash: atom_hash(b"awaitDrainWriters"),
    string: std::ptr::addr_of!(IMAGE_5.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_5);

#[used(linker)]
#[export_name = "perry_image_literal_0_636f726b6564"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_6: ImageString<7> = image(*b"corked\0");

wasm_string_image!(IMAGE_6);

#[used(linker)]
#[export_name = "perry_atom_636f726b6564"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_6: StaticAtom = StaticAtom {
    hash: atom_hash(b"corked"),
    string: std::ptr::addr_of!(IMAGE_6.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_6);

#[used(linker)]
#[export_name = "perry_image_literal_0_6f6e7772697465"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_7: ImageString<8> = image(*b"onwrite\0");

wasm_string_image!(IMAGE_7);

#[used(linker)]
#[export_name = "perry_atom_6f6e7772697465"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_7: StaticAtom = StaticAtom {
    hash: atom_hash(b"onwrite"),
    string: std::ptr::addr_of!(IMAGE_7.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_7);

#[used(linker)]
#[export_name = "perry_image_literal_0_77726974656c656e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_8: ImageString<9> = image(*b"writelen\0");

wasm_string_image!(IMAGE_8);

#[used(linker)]
#[export_name = "perry_atom_77726974656c656e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_8: StaticAtom = StaticAtom {
    hash: atom_hash(b"writelen"),
    string: std::ptr::addr_of!(IMAGE_8.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_8);

#[used(linker)]
#[export_name = "perry_image_literal_0_6275666665726564496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_9: ImageString<14> = image(*b"bufferedIndex\0");

wasm_string_image!(IMAGE_9);

#[used(linker)]
#[export_name = "perry_atom_6275666665726564496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_9: StaticAtom = StaticAtom {
    hash: atom_hash(b"bufferedIndex"),
    string: std::ptr::addr_of!(IMAGE_9.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_9);

#[used(linker)]
#[export_name = "perry_image_literal_0_70656e64696e676362"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_10: ImageString<10> = image(*b"pendingcb\0");

wasm_string_image!(IMAGE_10);

#[used(linker)]
#[export_name = "perry_atom_70656e64696e676362"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_10: StaticAtom = StaticAtom {
    hash: atom_hash(b"pendingcb"),
    string: std::ptr::addr_of!(IMAGE_10.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_10);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d732d656e74726965733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_11: ImageString<35> = image(*b"#<perry:url-search-params-entries>\0");

wasm_string_image!(IMAGE_11);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d732d656e74726965733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_11: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params-entries>"),
    string: std::ptr::addr_of!(IMAGE_11.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_11);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d732d6f776e65723e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_12: ImageString<33> = image(*b"#<perry:url-search-params-owner>\0");

wasm_string_image!(IMAGE_12);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d732d6f776e65723e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_12: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params-owner>"),
    string: std::ptr::addr_of!(IMAGE_12.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_12);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d687265663e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_13: ImageString<18> = image(*b"#<perry:url-href>\0");

wasm_string_image!(IMAGE_13);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d687265663e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_13: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-href>"),
    string: std::ptr::addr_of!(IMAGE_13.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_13);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d70726f746f636f6c3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_14: ImageString<22> = image(*b"#<perry:url-protocol>\0");

wasm_string_image!(IMAGE_14);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d70726f746f636f6c3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_14: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-protocol>"),
    string: std::ptr::addr_of!(IMAGE_14.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_14);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686f73743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_15: ImageString<18> = image(*b"#<perry:url-host>\0");

wasm_string_image!(IMAGE_15);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686f73743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_15: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-host>"),
    string: std::ptr::addr_of!(IMAGE_15.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_15);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686f73746e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_16: ImageString<22> = image(*b"#<perry:url-hostname>\0");

wasm_string_image!(IMAGE_16);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686f73746e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_16: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-hostname>"),
    string: std::ptr::addr_of!(IMAGE_16.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_16);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d706f72743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_17: ImageString<18> = image(*b"#<perry:url-port>\0");

wasm_string_image!(IMAGE_17);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d706f72743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_17: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-port>"),
    string: std::ptr::addr_of!(IMAGE_17.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_17);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d706174686e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_18: ImageString<22> = image(*b"#<perry:url-pathname>\0");

wasm_string_image!(IMAGE_18);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d706174686e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_18: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-pathname>"),
    string: std::ptr::addr_of!(IMAGE_18.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_18);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_19: ImageString<20> = image(*b"#<perry:url-search>\0");

wasm_string_image!(IMAGE_19);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_19: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search>"),
    string: std::ptr::addr_of!(IMAGE_19.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_19);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686173683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_20: ImageString<18> = image(*b"#<perry:url-hash>\0");

wasm_string_image!(IMAGE_20);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686173683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_20: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-hash>"),
    string: std::ptr::addr_of!(IMAGE_20.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_20);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d6f726967696e3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_21: ImageString<20> = image(*b"#<perry:url-origin>\0");

wasm_string_image!(IMAGE_21);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d6f726967696e3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_21: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-origin>"),
    string: std::ptr::addr_of!(IMAGE_21.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_21);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_22: ImageString<27> = image(*b"#<perry:url-search-params>\0");

wasm_string_image!(IMAGE_22);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_22: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params>"),
    string: std::ptr::addr_of!(IMAGE_22.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_22);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d757365726e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_23: ImageString<22> = image(*b"#<perry:url-username>\0");

wasm_string_image!(IMAGE_23);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d757365726e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_23: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-username>"),
    string: std::ptr::addr_of!(IMAGE_23.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_23);

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d70617373776f72643e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_24: ImageString<22> = image(*b"#<perry:url-password>\0");

wasm_string_image!(IMAGE_24);

#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d70617373776f72643e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_24: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-password>"),
    string: std::ptr::addr_of!(IMAGE_24.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_24);

#[used(linker)]
#[export_name = "perry_image_literal_0_76616c7565"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_25: ImageString<6> = image(*b"value\0");

wasm_string_image!(IMAGE_25);

#[used(linker)]
#[export_name = "perry_atom_76616c7565"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_25: StaticAtom = StaticAtom {
    hash: atom_hash(b"value"),
    string: std::ptr::addr_of!(IMAGE_25.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_25);

#[used(linker)]
#[export_name = "perry_image_literal_0_646f6e65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_26: ImageString<5> = image(*b"done\0");

wasm_string_image!(IMAGE_26);

#[used(linker)]
#[export_name = "perry_atom_646f6e65"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_26: StaticAtom = StaticAtom {
    hash: atom_hash(b"done"),
    string: std::ptr::addr_of!(IMAGE_26.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_26);

#[used(linker)]
#[export_name = "perry_image_literal_0_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_strings"
)]
pub(super) static IMAGE_27: ImageString<6> = image(*b"index\0");

wasm_string_image!(IMAGE_27);

#[used(linker)]
#[export_name = "perry_atom_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_27: StaticAtom = StaticAtom {
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
pub(super) static IMAGE_28: ImageString<6> = image(*b"input\0");

wasm_string_image!(IMAGE_28);

#[used(linker)]
#[export_name = "perry_atom_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_28: StaticAtom = StaticAtom {
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
pub(super) static IMAGE_29: ImageString<7> = image(*b"groups\0");

wasm_string_image!(IMAGE_29);

#[used(linker)]
#[export_name = "perry_atom_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_29: StaticAtom = StaticAtom {
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
pub(super) static IMAGE_30: ImageString<8> = image(*b"indices\0");

wasm_string_image!(IMAGE_30);

#[used(linker)]
#[export_name = "perry_atom_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
    link_section = "perry_atoms"
)]
pub(super) static ATOM_30: StaticAtom = StaticAtom {
    hash: atom_hash(b"indices"),
    string: std::ptr::addr_of!(IMAGE_30.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

wasm_atom!(ATOM_30);
