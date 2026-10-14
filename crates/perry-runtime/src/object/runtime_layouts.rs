//! Runtime declarations using the step-0 ABI, shared with emitted code.
use super::layout_desc::{PerryLayoutDesc, PerryLayoutKey, StaticAtom};
use crate::gc::GcHeader;
use crate::string::StringHeader;

#[repr(C, align(8))]
struct ImageString<const N: usize> {
    gc: GcHeader,
    string: StringHeader,
    bytes: [u8; N],
}

const fn image<const N: usize>(bytes: [u8; N]) -> ImageString<N> {
    let len = (N - 1) as u32;
    ImageString {
        gc: GcHeader {
            obj_type: crate::gc::GC_TYPE_STRING,
            gc_flags: crate::gc::GC_FLAG_MARKED
                | crate::gc::GC_FLAG_PINNED
                | crate::gc::GC_FLAG_INTERNED,
            _reserved: crate::string::STRING_IMAGE_MARKER,
            size: 28 + N as u32,
        },
        string: StringHeader {
            utf16_len: len,
            byte_len: len,
            capacity: len,
            refcount: 0,
            flags: crate::string::STRING_FLAG_WTF8_VALIDATED,
        },
        bytes,
    }
}

pub(crate) const fn atom_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut i = 0;
    while i < bytes.len() {
        hash = (hash ^ bytes[i] as u64).wrapping_mul(0x0100_0000_01b3);
        i += 1;
    }
    hash
}

#[inline]
pub(crate) unsafe fn layout_id(desc: *const PerryLayoutDesc) -> u32 {
    (*desc).layout_id
}

/// The existing shape record owns the per-agent builtin prototype word.
pub(crate) unsafe fn bind_builtin_prototype(desc: *const PerryLayoutDesc, bits: u64) {
    let record = super::shapes::shape_descriptor_by_id(layout_id(desc))
        .expect("layout registered before builtin creation");
    super::shapes::write_identity_word(record.proto_id, bits);
}

#[used(linker)]
#[export_name = "perry_image_literal_0_6869676857617465724d61726b"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_0: ImageString<14> = image(*b"highWaterMark\0");
#[used(linker)]
#[export_name = "perry_atom_6869676857617465724d61726b"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_0: StaticAtom = StaticAtom {
    hash: atom_hash(b"highWaterMark"),
    string: std::ptr::addr_of!(IMAGE_0.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_627566666572"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_1: ImageString<7> = image(*b"buffer\0");
#[used(linker)]
#[export_name = "perry_atom_627566666572"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_1: StaticAtom = StaticAtom {
    hash: atom_hash(b"buffer"),
    string: std::ptr::addr_of!(IMAGE_1.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_627566666572496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_2: ImageString<12> = image(*b"bufferIndex\0");
#[used(linker)]
#[export_name = "perry_atom_627566666572496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_2: StaticAtom = StaticAtom {
    hash: atom_hash(b"bufferIndex"),
    string: std::ptr::addr_of!(IMAGE_2.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_6c656e677468"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_3: ImageString<7> = image(*b"length\0");
#[used(linker)]
#[export_name = "perry_atom_6c656e677468"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_3: StaticAtom = StaticAtom {
    hash: atom_hash(b"length"),
    string: std::ptr::addr_of!(IMAGE_3.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_7069706573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_4: ImageString<6> = image(*b"pipes\0");
#[used(linker)]
#[export_name = "perry_atom_7069706573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_4: StaticAtom = StaticAtom {
    hash: atom_hash(b"pipes"),
    string: std::ptr::addr_of!(IMAGE_4.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_6177616974447261696e57726974657273"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_5: ImageString<18> = image(*b"awaitDrainWriters\0");
#[used(linker)]
#[export_name = "perry_atom_6177616974447261696e57726974657273"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_5: StaticAtom = StaticAtom {
    hash: atom_hash(b"awaitDrainWriters"),
    string: std::ptr::addr_of!(IMAGE_5.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_636f726b6564"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_6: ImageString<7> = image(*b"corked\0");
#[used(linker)]
#[export_name = "perry_atom_636f726b6564"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_6: StaticAtom = StaticAtom {
    hash: atom_hash(b"corked"),
    string: std::ptr::addr_of!(IMAGE_6.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_6f6e7772697465"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_7: ImageString<8> = image(*b"onwrite\0");
#[used(linker)]
#[export_name = "perry_atom_6f6e7772697465"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_7: StaticAtom = StaticAtom {
    hash: atom_hash(b"onwrite"),
    string: std::ptr::addr_of!(IMAGE_7.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_77726974656c656e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_8: ImageString<9> = image(*b"writelen\0");
#[used(linker)]
#[export_name = "perry_atom_77726974656c656e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_8: StaticAtom = StaticAtom {
    hash: atom_hash(b"writelen"),
    string: std::ptr::addr_of!(IMAGE_8.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_6275666665726564496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_9: ImageString<14> = image(*b"bufferedIndex\0");
#[used(linker)]
#[export_name = "perry_atom_6275666665726564496e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_9: StaticAtom = StaticAtom {
    hash: atom_hash(b"bufferedIndex"),
    string: std::ptr::addr_of!(IMAGE_9.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_70656e64696e676362"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_10: ImageString<10> = image(*b"pendingcb\0");
#[used(linker)]
#[export_name = "perry_atom_70656e64696e676362"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_10: StaticAtom = StaticAtom {
    hash: atom_hash(b"pendingcb"),
    string: std::ptr::addr_of!(IMAGE_10.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_5f656e7472696573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_11: ImageString<9> = image(*b"_entries\0");
#[used(linker)]
#[export_name = "perry_atom_5f656e7472696573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_11: StaticAtom = StaticAtom {
    hash: atom_hash(b"_entries"),
    string: std::ptr::addr_of!(IMAGE_11.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_5f6f776e6572"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_12: ImageString<7> = image(*b"_owner\0");
#[used(linker)]
#[export_name = "perry_atom_5f6f776e6572"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_12: StaticAtom = StaticAtom {
    hash: atom_hash(b"_owner"),
    string: std::ptr::addr_of!(IMAGE_12.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d687265663e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_13: ImageString<18> = image(*b"#<perry:url-href>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d687265663e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_13: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-href>"),
    string: std::ptr::addr_of!(IMAGE_13.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d70726f746f636f6c3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_14: ImageString<22> = image(*b"#<perry:url-protocol>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d70726f746f636f6c3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_14: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-protocol>"),
    string: std::ptr::addr_of!(IMAGE_14.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686f73743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_15: ImageString<18> = image(*b"#<perry:url-host>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686f73743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_15: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-host>"),
    string: std::ptr::addr_of!(IMAGE_15.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686f73746e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_16: ImageString<22> = image(*b"#<perry:url-hostname>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686f73746e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_16: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-hostname>"),
    string: std::ptr::addr_of!(IMAGE_16.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d706f72743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_17: ImageString<18> = image(*b"#<perry:url-port>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d706f72743e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_17: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-port>"),
    string: std::ptr::addr_of!(IMAGE_17.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d706174686e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_18: ImageString<22> = image(*b"#<perry:url-pathname>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d706174686e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_18: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-pathname>"),
    string: std::ptr::addr_of!(IMAGE_18.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_19: ImageString<20> = image(*b"#<perry:url-search>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_19: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search>"),
    string: std::ptr::addr_of!(IMAGE_19.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d686173683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_20: ImageString<18> = image(*b"#<perry:url-hash>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d686173683e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_20: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-hash>"),
    string: std::ptr::addr_of!(IMAGE_20.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d6f726967696e3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_21: ImageString<20> = image(*b"#<perry:url-origin>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d6f726967696e3e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_21: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-origin>"),
    string: std::ptr::addr_of!(IMAGE_21.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d7365617263682d706172616d733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_22: ImageString<27> = image(*b"#<perry:url-search-params>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d7365617263682d706172616d733e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_22: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-search-params>"),
    string: std::ptr::addr_of!(IMAGE_22.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d757365726e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_23: ImageString<22> = image(*b"#<perry:url-username>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d757365726e616d653e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_23: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-username>"),
    string: std::ptr::addr_of!(IMAGE_23.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_233c70657272793a75726c2d70617373776f72643e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_24: ImageString<22> = image(*b"#<perry:url-password>\0");
#[used(linker)]
#[export_name = "perry_atom_233c70657272793a75726c2d70617373776f72643e"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_24: StaticAtom = StaticAtom {
    hash: atom_hash(b"#<perry:url-password>"),
    string: std::ptr::addr_of!(IMAGE_24.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_76616c7565"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_25: ImageString<6> = image(*b"value\0");
#[used(linker)]
#[export_name = "perry_atom_76616c7565"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_25: StaticAtom = StaticAtom {
    hash: atom_hash(b"value"),
    string: std::ptr::addr_of!(IMAGE_25.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

#[used(linker)]
#[export_name = "perry_image_literal_0_646f6e65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_26: ImageString<5> = image(*b"done\0");
#[used(linker)]
#[export_name = "perry_atom_646f6e65"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_26: StaticAtom = StaticAtom {
    hash: atom_hash(b"done"),
    string: std::ptr::addr_of!(IMAGE_26.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};

static READABLE_STATE_KEYS: [PerryLayoutKey; 6] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_0),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_1),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_2),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_3),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_4),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_5),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
];
static READABLE_STATE_INDEX: [u32; 16] = [
    u32::MAX,
    2,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    3,
    u32::MAX,
    5,
    u32::MAX,
    0,
    u32::MAX,
    1,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut READABLE_STATE: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540001,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 6,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 3,
    keys: READABLE_STATE_KEYS.as_ptr(),
    key_count: 6,
    index_mask: 15,
    index: READABLE_STATE_INDEX.as_ptr(),
};

static WRITABLE_STATE_KEYS: [PerryLayoutKey; 7] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_0),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_3),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_6),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_7),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_8),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_9),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_10),
        slot: 6,
        attrs: 0,
        _pad: [0; 3],
    },
];
static WRITABLE_STATE_INDEX: [u32; 16] = [
    6,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    1,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    0,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    2,
    5,
    3,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut WRITABLE_STATE: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540002,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 7,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 4,
    keys: WRITABLE_STATE_KEYS.as_ptr(),
    key_count: 7,
    index_mask: 15,
    index: WRITABLE_STATE_INDEX.as_ptr(),
};

static URL_SEARCH_PARAMS_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_11),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_12),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static URL_SEARCH_PARAMS_INDEX: [u32; 4] = [u32::MAX, 1, 0, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut URL_SEARCH_PARAMS: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540003,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 0,
    keys: URL_SEARCH_PARAMS_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: URL_SEARCH_PARAMS_INDEX.as_ptr(),
};

static URL_COMPONENTS_KEYS: [PerryLayoutKey; 12] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_13),
        slot: 0,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_14),
        slot: 1,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_15),
        slot: 2,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_16),
        slot: 3,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_17),
        slot: 4,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_18),
        slot: 5,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_19),
        slot: 6,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_20),
        slot: 7,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_21),
        slot: 8,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_22),
        slot: 9,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_23),
        slot: 10,
        attrs: 66,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_24),
        slot: 11,
        attrs: 66,
        _pad: [0; 3],
    },
];
static URL_COMPONENTS_INDEX: [u32; 32] = [
    3,
    6,
    u32::MAX,
    9,
    7,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    1,
    5,
    u32::MAX,
    u32::MAX,
    10,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    2,
    8,
    u32::MAX,
    4,
    u32::MAX,
    11,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    0,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut URL_COMPONENTS: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540004,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 12,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 2,
    keys: URL_COMPONENTS_KEYS.as_ptr(),
    key_count: 12,
    index_mask: 31,
    index: URL_COMPONENTS_INDEX.as_ptr(),
};

static ITERATOR_RESULT_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_25),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_26),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static ITERATOR_RESULT_INDEX: [u32; 4] = [u32::MAX, 1, 0, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ITERATOR_RESULT: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540005,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 0,
    keys: ITERATOR_RESULT_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: ITERATOR_RESULT_INDEX.as_ptr(),
};

static ITERATOR_RESULT_DONE_FIRST_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_26),
        slot: 0,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(ATOM_25),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static ITERATOR_RESULT_DONE_FIRST_INDEX: [u32; 4] = [u32::MAX, 0, 1, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ITERATOR_RESULT_DONE_FIRST: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540006,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 0,
    keys: ITERATOR_RESULT_DONE_FIRST_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: ITERATOR_RESULT_DONE_FIRST_INDEX.as_ptr(),
};

static TIMEOUT_KEYS: [PerryLayoutKey; 0] = [];
static TIMEOUT_INDEX: [u32; 2] = [u32::MAX, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut TIMEOUT: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x52540007,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 0,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 5,
    keys: TIMEOUT_KEYS.as_ptr(),
    key_count: 0,
    index_mask: 1,
    index: TIMEOUT_INDEX.as_ptr(),
};

static IMMEDIATE_KEYS: [PerryLayoutKey; 0] = [];
static IMMEDIATE_INDEX: [u32; 2] = [u32::MAX, u32::MAX];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
    proto: 6,
    keys: IMMEDIATE_KEYS.as_ptr(),
    key_count: 0,
    index_mask: 1,
    index: IMMEDIATE_INDEX.as_ptr(),
};

pub(crate) const URL_CLASS_ID: u32 = 0xFFFF0063;

mod stream_prototypes;
pub(crate) use stream_prototypes::*;
#[used(linker)]
#[export_name = "perry_image_literal_0_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_27: ImageString<6> = image(*b"index\0");
#[used(linker)]
#[export_name = "perry_atom_696e646578"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_27: StaticAtom = StaticAtom {
    hash: atom_hash(b"index"),
    string: std::ptr::addr_of!(IMAGE_27.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
#[used(linker)]
#[export_name = "perry_image_literal_0_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_28: ImageString<6> = image(*b"input\0");
#[used(linker)]
#[export_name = "perry_atom_696e707574"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_28: StaticAtom = StaticAtom {
    hash: atom_hash(b"input"),
    string: std::ptr::addr_of!(IMAGE_28.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
#[used(linker)]
#[export_name = "perry_image_literal_0_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_29: ImageString<7> = image(*b"groups\0");
#[used(linker)]
#[export_name = "perry_atom_67726f757073"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_29: StaticAtom = StaticAtom {
    hash: atom_hash(b"groups"),
    string: std::ptr::addr_of!(IMAGE_29.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
#[used(linker)]
#[export_name = "perry_image_literal_0_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static IMAGE_30: ImageString<8> = image(*b"indices\0");
#[used(linker)]
#[export_name = "perry_atom_696e6469636573"]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_atoms"
)]
static ATOM_30: StaticAtom = StaticAtom {
    hash: atom_hash(b"indices"),
    string: std::ptr::addr_of!(IMAGE_30.string),
    kind: crate::codegen_abi::ATOM_KIND_STRING,
    _pad: [0; 3],
    index: 0,
};
/// `Symbol.iterator` as a layout key (§6a, a well-known symbol atom).
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
    not(any(target_vendor = "apple", target_os = "windows")),
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
    proto: 0,
    keys: REGEX_EXEC_NAMED_KEYS.as_ptr(),
    key_count: 4,
    index_mask: 7,
    index: REGEX_EXEC_NAMED_INDEX.as_ptr(),
};
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
    not(any(target_vendor = "apple", target_os = "windows")),
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
    proto: 0,
    keys: REGEX_EXEC_INDICES_NAMED_KEYS.as_ptr(),
    key_count: 5,
    index_mask: 15,
    index: REGEX_EXEC_INDICES_NAMED_INDEX.as_ptr(),
};
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
    not(any(target_vendor = "apple", target_os = "windows")),
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
    proto: 0,
    keys: REGEX_INDICES_GROUPS_NAMED_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: REGEX_INDICES_GROUPS_NAMED_INDEX.as_ptr(),
};
static ARRAY_PROTOTYPE_NAMED_KEYS: [PerryLayoutKey; 1] = [PerryLayoutKey {
    atom: std::ptr::addr_of!(ATOM_SYMBOL_ITERATOR),
    slot: 0,
    attrs: 0,
    _pad: [0; 3],
}];
static ARRAY_PROTOTYPE_NAMED_INDEX: [u32; 2] = [u32::MAX, 0];
/// `%Array.prototype%`'s named layout (`ArrayHeader.named`, object model
/// step 7): `Symbol.iterator` is its fixed slot 0, so array iteration reads
/// one slot. The builtin methods population installs follow it inline: the
/// layout reserves the builtin prototype room.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut ARRAY_PROTOTYPE_NAMED: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x5254000C,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: crate::object::BUILTIN_PROTOTYPE_ROOM,
    layout_id: super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: 0,
    keys: ARRAY_PROTOTYPE_NAMED_KEYS.as_ptr(),
    key_count: 1,
    index_mask: 1,
    index: ARRAY_PROTOTYPE_NAMED_INDEX.as_ptr(),
};
/// The fixed slot of `Symbol.iterator` in [`ARRAY_PROTOTYPE_NAMED`].
pub(crate) const ARRAY_PROTOTYPE_ITERATOR_SLOT: u32 = 0;

/// Empty array named family; absent iterator slot 0 is the hole marker.
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
    proto: 0,
    keys: ARRAY_PROTOTYPE_NAMED_KEYS.as_ptr(),
    key_count: 1,
    index_mask: 1,
    index: ARRAY_PROTOTYPE_NAMED_INDEX.as_ptr(),
};

// Fixed legacy SQLite column metadata; the five data properties are all present.
#[used(linker)]
#[export_name = "perry_image_literal_0_636f6c756d6e"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_0: ImageString<7> = image(*b"column\0");
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
#[used(linker)]
#[export_name = "perry_image_literal_0_6461746162617365"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_1: ImageString<9> = image(*b"database\0");
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
#[used(linker)]
#[export_name = "perry_image_literal_0_6e616d65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_2: ImageString<5> = image(*b"name\0");
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
#[used(linker)]
#[export_name = "perry_image_literal_0_7461626c65"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_3: ImageString<6> = image(*b"table\0");
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
#[used(linker)]
#[export_name = "perry_image_literal_0_74797065"]
#[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
#[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_strings"
)]
static SQLITE_IMAGE_4: ImageString<5> = image(*b"type\0");
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
#[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
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
    not(any(target_vendor = "apple", target_os = "windows")),
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
    proto: 0,
    keys: SQLITE_COLUMN_KEYS.as_ptr(),
    key_count: 5,
    index_mask: 15,
    index: SQLITE_COLUMN_INDEX.as_ptr(),
};
