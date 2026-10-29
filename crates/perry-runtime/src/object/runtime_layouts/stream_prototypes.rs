//! Fixed string-only stream prototype declarations.
use super::*;

macro_rules! atom {
    ($string:ident, $atom:ident, $name:literal, $bytes:literal, $image_export:literal, $atom_export:literal) => {
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
        #[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
            link_section = "perry_strings"
        )]
        #[export_name = $image_export]
        static $string: ImageString<{ $bytes.len() }> = image(*$bytes);
        wasm_string_image!($string);
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
        #[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
            link_section = "perry_atoms"
        )]
        #[export_name = $atom_export]
        static $atom: StaticAtom = StaticAtom {
            hash: atom_hash($name),
            string: std::ptr::addr_of!($string.string),
            kind: crate::codegen_abi::ATOM_KIND_STRING,
            _pad: [0; 3],
            index: 0,
        };
        wasm_atom!($atom);
    };
}
atom!(
    S0,
    A0,
    b"objectMode",
    b"objectMode\0",
    "perry_image_literal_0_6f626a6563744d6f6465",
    "perry_atom_6f626a6563744d6f6465"
);
atom!(
    S1,
    A1,
    b"pipesCount",
    b"pipesCount\0",
    "perry_image_literal_0_7069706573436f756e74",
    "perry_atom_7069706573436f756e74"
);
atom!(
    S2,
    A2,
    b"flowing",
    b"flowing\0",
    "perry_image_literal_0_666c6f77696e67",
    "perry_atom_666c6f77696e67"
);
atom!(
    S3,
    A3,
    b"ended",
    b"ended\0",
    "perry_image_literal_0_656e646564",
    "perry_atom_656e646564"
);
atom!(
    S4,
    A4,
    b"endEmitted",
    b"endEmitted\0",
    "perry_image_literal_0_656e64456d6974746564",
    "perry_atom_656e64456d6974746564"
);
atom!(
    S5,
    A5,
    b"readableListening",
    b"readableListening\0",
    "perry_image_literal_0_7265616461626c654c697374656e696e67",
    "perry_atom_7265616461626c654c697374656e696e67"
);
atom!(
    S6,
    A6,
    b"resumeScheduled",
    b"resumeScheduled\0",
    "perry_image_literal_0_726573756d655363686564756c6564",
    "perry_atom_726573756d655363686564756c6564"
);
atom!(
    S7,
    A7,
    b"errorEmitted",
    b"errorEmitted\0",
    "perry_image_literal_0_6572726f72456d6974746564",
    "perry_atom_6572726f72456d6974746564"
);
atom!(
    S8,
    A8,
    b"emitClose",
    b"emitClose\0",
    "perry_image_literal_0_656d6974436c6f7365",
    "perry_atom_656d6974436c6f7365"
);
atom!(
    S9,
    A9,
    b"autoDestroy",
    b"autoDestroy\0",
    "perry_image_literal_0_6175746f44657374726f79",
    "perry_atom_6175746f44657374726f79"
);
atom!(
    S10,
    A10,
    b"destroyed",
    b"destroyed\0",
    "perry_image_literal_0_64657374726f796564",
    "perry_atom_64657374726f796564"
);
atom!(
    S11,
    A11,
    b"closed",
    b"closed\0",
    "perry_image_literal_0_636c6f736564",
    "perry_atom_636c6f736564"
);
atom!(
    S12,
    A12,
    b"closeEmitted",
    b"closeEmitted\0",
    "perry_image_literal_0_636c6f7365456d6974746564",
    "perry_atom_636c6f7365456d6974746564"
);
atom!(
    S13,
    A13,
    b"dataEmitted",
    b"dataEmitted\0",
    "perry_image_literal_0_64617461456d6974746564",
    "perry_atom_64617461456d6974746564"
);
atom!(
    S14,
    A14,
    b"errored",
    b"errored\0",
    "perry_image_literal_0_6572726f726564",
    "perry_atom_6572726f726564"
);
atom!(
    S15,
    A15,
    b"encoding",
    b"encoding\0",
    "perry_image_literal_0_656e636f64696e67",
    "perry_atom_656e636f64696e67"
);
atom!(
    S16,
    A16,
    b"finalCalled",
    b"finalCalled\0",
    "perry_image_literal_0_66696e616c43616c6c6564",
    "perry_atom_66696e616c43616c6c6564"
);
atom!(
    S17,
    A17,
    b"needDrain",
    b"needDrain\0",
    "perry_image_literal_0_6e656564447261696e",
    "perry_atom_6e656564447261696e"
);
atom!(
    S18,
    A18,
    b"ending",
    b"ending\0",
    "perry_image_literal_0_656e64696e67",
    "perry_atom_656e64696e67"
);
atom!(
    S19,
    A19,
    b"finished",
    b"finished\0",
    "perry_image_literal_0_66696e6973686564",
    "perry_atom_66696e6973686564"
);
atom!(
    S20,
    A20,
    b"decodeStrings",
    b"decodeStrings\0",
    "perry_image_literal_0_6465636f6465537472696e6773",
    "perry_atom_6465636f6465537472696e6773"
);
atom!(
    S21,
    A21,
    b"defaultEncoding",
    b"defaultEncoding\0",
    "perry_image_literal_0_64656661756c74456e636f64696e67",
    "perry_atom_64656661756c74456e636f64696e67"
);
atom!(
    S22,
    A22,
    b"writable",
    b"writable\0",
    "perry_image_literal_0_7772697461626c65",
    "perry_atom_7772697461626c65"
);
atom!(
    S23,
    A23,
    b"constructor",
    b"constructor\0",
    "perry_image_literal_0_636f6e7374727563746f72",
    "perry_atom_636f6e7374727563746f72"
);
atom!(
    S46,
    A46,
    b"emit",
    b"emit\0",
    "perry_image_literal_0_656d6974",
    "perry_atom_656d6974"
);
atom!(
    S56,
    A56,
    b"emitDestroy",
    b"emitDestroy\0",
    "perry_image_literal_0_656d697444657374726f79",
    "perry_atom_656d697444657374726f79"
);
atom!(
    S57,
    A57,
    b"asyncId",
    b"asyncId\0",
    "perry_image_literal_0_6173796e634964",
    "perry_atom_6173796e634964"
);
atom!(
    S58,
    A58,
    b"triggerAsyncId",
    b"triggerAsyncId\0",
    "perry_image_literal_0_747269676765724173796e634964",
    "perry_atom_747269676765724173796e634964"
);
atom!(
    S59,
    A59,
    b"asyncResource",
    b"asyncResource\0",
    "perry_image_literal_0_6173796e635265736f75726365",
    "perry_atom_6173796e635265736f75726365"
);

runtime_layout!(READABLE_STATE_PROTO, 0x53500001, 0, crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [A0 => 58, A1 => 58, A2 => 58, A3 => 58, A4 => 58, A5 => 58, A6 => 58, A7 => 58, A8 => 58, A9 => 58, A10 => 58, A11 => 58, A12 => 58, A13 => 58, A14 => 58, A15 => 58], realm = ["node:stream.Readable", "node:stream.Duplex", "node:stream.Transform", "node:stream.PassThrough"]);
runtime_layout!(WRITABLE_STATE_PROTO, 0x53500002, 0, crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [A0 => 58, A16 => 58, A17 => 58, A18 => 58, A3 => 58, A19 => 58, A10 => 58, A20 => 58, A7 => 58, A8 => 58, A9 => 58, A11 => 58, A12 => 58, A14 => 58, A21 => 58, A22 => 58], realm = ["node:stream.Writable", "node:stream.Duplex", "node:stream.Transform", "node:stream.PassThrough"]);
runtime_layout!(EE_ASYNC_RESOURCE_PROTO, 0x53500008, 0, crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [A23 => 2, A46 => 2, A56 => 2, A57 => 26, A58 => 26, A59 => 26], realm = ["node:events.EventEmitterAsyncResource.prototype"]);

#[cfg(test)]
mod source_equivalence_tests {
    use super::*;
    #[test]
    fn legacy_stream_prototype_descriptor_facts_survive_single_source_migration() {
        unsafe {
            super::super::source_equivalence_tests::check(
                std::ptr::addr_of!(READABLE_STATE_PROTO),
                "READABLE_STATE_PROTO",
                0x53500001,
                crate::codegen_abi::native_layout_word("READABLE_STATE_PROTO", 0),
                crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
                &[
                    (A0.hash, 0, 58),
                    (A1.hash, 1, 58),
                    (A2.hash, 2, 58),
                    (A3.hash, 3, 58),
                    (A4.hash, 4, 58),
                    (A5.hash, 5, 58),
                    (A6.hash, 6, 58),
                    (A7.hash, 7, 58),
                    (A8.hash, 8, 58),
                    (A9.hash, 9, 58),
                    (A10.hash, 10, 58),
                    (A11.hash, 11, 58),
                    (A12.hash, 12, 58),
                    (A13.hash, 13, 58),
                    (A14.hash, 14, 58),
                    (A15.hash, 15, 58),
                ],
                &[
                    u32::MAX,
                    0,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    11,
                    12,
                    u32::MAX,
                    9,
                    14,
                    4,
                    2,
                    10,
                    13,
                    u32::MAX,
                    u32::MAX,
                    15,
                    6,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    8,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    5,
                    3,
                    u32::MAX,
                    1,
                    u32::MAX,
                    7,
                ],
            );
            super::super::source_equivalence_tests::check(
                std::ptr::addr_of!(WRITABLE_STATE_PROTO),
                "WRITABLE_STATE_PROTO",
                0x53500002,
                crate::codegen_abi::native_layout_word("WRITABLE_STATE_PROTO", 0),
                crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
                &[
                    (A0.hash, 0, 58),
                    (A16.hash, 1, 58),
                    (A17.hash, 2, 58),
                    (A18.hash, 3, 58),
                    (A3.hash, 4, 58),
                    (A19.hash, 5, 58),
                    (A10.hash, 6, 58),
                    (A20.hash, 7, 58),
                    (A7.hash, 8, 58),
                    (A8.hash, 9, 58),
                    (A9.hash, 10, 58),
                    (A11.hash, 11, 58),
                    (A12.hash, 12, 58),
                    (A14.hash, 13, 58),
                    (A21.hash, 14, 58),
                    (A22.hash, 15, 58),
                ],
                &[
                    u32::MAX,
                    0,
                    u32::MAX,
                    5,
                    u32::MAX,
                    11,
                    1,
                    12,
                    10,
                    13,
                    u32::MAX,
                    7,
                    6,
                    u32::MAX,
                    u32::MAX,
                    14,
                    u32::MAX,
                    2,
                    15,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    3,
                    9,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    4,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    8,
                ],
            );
            super::super::source_equivalence_tests::check(
                std::ptr::addr_of!(EE_ASYNC_RESOURCE_PROTO),
                "EE_ASYNC_RESOURCE_PROTO",
                0x53500008,
                crate::codegen_abi::native_layout_word("EE_ASYNC_RESOURCE_PROTO", 0),
                crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
                &[
                    (A23.hash, 0, 2),
                    (A46.hash, 1, 2),
                    (A56.hash, 2, 2),
                    (A57.hash, 3, 26),
                    (A58.hash, 4, 26),
                    (A59.hash, 5, 26),
                ],
                &[
                    4,
                    u32::MAX,
                    3,
                    u32::MAX,
                    2,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    1,
                    5,
                    u32::MAX,
                    u32::MAX,
                    u32::MAX,
                    0,
                ],
            );
        }
    }
}
