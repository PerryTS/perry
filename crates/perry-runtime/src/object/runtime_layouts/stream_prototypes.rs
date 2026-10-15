//! Fixed string-only stream prototype declarations.
use super::*;

macro_rules! atom {
    ($string:ident, $atom:ident, $name:literal, $bytes:literal, $image_export:literal, $atom_export:literal) => {
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
        #[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows")),
            link_section = "perry_strings"
        )]
        #[export_name = $image_export]
        static $string: ImageString<{ $bytes.len() }> = image(*$bytes);
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_atoms")]
        #[cfg_attr(target_os = "windows", link_section = ".perryatm$m")]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows")),
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
    S24,
    A24,
    b"write",
    b"write\0",
    "perry_image_literal_0_7772697465",
    "perry_atom_7772697465"
);
atom!(
    S25,
    A25,
    b"end",
    b"end\0",
    "perry_image_literal_0_656e64",
    "perry_atom_656e64"
);
atom!(
    S26,
    A26,
    b"cork",
    b"cork\0",
    "perry_image_literal_0_636f726b",
    "perry_atom_636f726b"
);
atom!(
    S27,
    A27,
    b"uncork",
    b"uncork\0",
    "perry_image_literal_0_756e636f726b",
    "perry_atom_756e636f726b"
);
atom!(
    S28,
    A28,
    b"destroy",
    b"destroy\0",
    "perry_image_literal_0_64657374726f79",
    "perry_atom_64657374726f79"
);
atom!(
    S29,
    A29,
    b"setDefaultEncoding",
    b"setDefaultEncoding\0",
    "perry_image_literal_0_73657444656661756c74456e636f64696e67",
    "perry_atom_73657444656661756c74456e636f64696e67"
);
atom!(
    S30,
    A30,
    b"writableObjectMode",
    b"writableObjectMode\0",
    "perry_image_literal_0_7772697461626c654f626a6563744d6f6465",
    "perry_atom_7772697461626c654f626a6563744d6f6465"
);
atom!(
    S31,
    A31,
    b"writableHighWaterMark",
    b"writableHighWaterMark\0",
    "perry_image_literal_0_7772697461626c654869676857617465724d61726b",
    "perry_atom_7772697461626c654869676857617465724d61726b"
);
atom!(
    S32,
    A32,
    b"writableLength",
    b"writableLength\0",
    "perry_image_literal_0_7772697461626c654c656e677468",
    "perry_atom_7772697461626c654c656e677468"
);
atom!(
    S33,
    A33,
    b"writableNeedDrain",
    b"writableNeedDrain\0",
    "perry_image_literal_0_7772697461626c654e656564447261696e",
    "perry_atom_7772697461626c654e656564447261696e"
);
atom!(
    S34,
    A34,
    b"writableCorked",
    b"writableCorked\0",
    "perry_image_literal_0_7772697461626c65436f726b6564",
    "perry_atom_7772697461626c65436f726b6564"
);
atom!(
    S35,
    A35,
    b"writableEnded",
    b"writableEnded\0",
    "perry_image_literal_0_7772697461626c65456e646564",
    "perry_atom_7772697461626c65456e646564"
);
atom!(
    S36,
    A36,
    b"writableFinished",
    b"writableFinished\0",
    "perry_image_literal_0_7772697461626c6546696e6973686564",
    "perry_atom_7772697461626c6546696e6973686564"
);
atom!(
    S37,
    A37,
    b"_transform",
    b"_transform\0",
    "perry_image_literal_0_5f7472616e73666f726d",
    "perry_atom_5f7472616e73666f726d"
);
atom!(
    S38,
    A38,
    b"on",
    b"on\0",
    "perry_image_literal_0_6f6e",
    "perry_atom_6f6e"
);
atom!(
    S39,
    A39,
    b"once",
    b"once\0",
    "perry_image_literal_0_6f6e6365",
    "perry_atom_6f6e6365"
);
atom!(
    S40,
    A40,
    b"prependListener",
    b"prependListener\0",
    "perry_image_literal_0_70726570656e644c697374656e6572",
    "perry_atom_70726570656e644c697374656e6572"
);
atom!(
    S41,
    A41,
    b"prependOnceListener",
    b"prependOnceListener\0",
    "perry_image_literal_0_70726570656e644f6e63654c697374656e6572",
    "perry_atom_70726570656e644f6e63654c697374656e6572"
);
atom!(
    S42,
    A42,
    b"off",
    b"off\0",
    "perry_image_literal_0_6f6666",
    "perry_atom_6f6666"
);
atom!(
    S43,
    A43,
    b"addListener",
    b"addListener\0",
    "perry_image_literal_0_6164644c697374656e6572",
    "perry_atom_6164644c697374656e6572"
);
atom!(
    S44,
    A44,
    b"removeListener",
    b"removeListener\0",
    "perry_image_literal_0_72656d6f76654c697374656e6572",
    "perry_atom_72656d6f76654c697374656e6572"
);
atom!(
    S45,
    A45,
    b"removeAllListeners",
    b"removeAllListeners\0",
    "perry_image_literal_0_72656d6f7665416c6c4c697374656e657273",
    "perry_atom_72656d6f7665416c6c4c697374656e657273"
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
    S47,
    A47,
    b"setMaxListeners",
    b"setMaxListeners\0",
    "perry_image_literal_0_7365744d61784c697374656e657273",
    "perry_atom_7365744d61784c697374656e657273"
);
atom!(
    S48,
    A48,
    b"getMaxListeners",
    b"getMaxListeners\0",
    "perry_image_literal_0_6765744d61784c697374656e657273",
    "perry_atom_6765744d61784c697374656e657273"
);
atom!(
    S49,
    A49,
    b"eventNames",
    b"eventNames\0",
    "perry_image_literal_0_6576656e744e616d6573",
    "perry_atom_6576656e744e616d6573"
);
atom!(
    S50,
    A50,
    b"listenerCount",
    b"listenerCount\0",
    "perry_image_literal_0_6c697374656e6572436f756e74",
    "perry_atom_6c697374656e6572436f756e74"
);
atom!(
    S51,
    A51,
    b"listeners",
    b"listeners\0",
    "perry_image_literal_0_6c697374656e657273",
    "perry_atom_6c697374656e657273"
);
atom!(
    S52,
    A52,
    b"rawListeners",
    b"rawListeners\0",
    "perry_image_literal_0_7261774c697374656e657273",
    "perry_atom_7261774c697374656e657273"
);
atom!(
    S53,
    A53,
    b"_events",
    b"_events\0",
    "perry_image_literal_0_5f6576656e7473",
    "perry_atom_5f6576656e7473"
);
atom!(
    S54,
    A54,
    b"_eventsCount",
    b"_eventsCount\0",
    "perry_image_literal_0_5f6576656e7473436f756e74",
    "perry_atom_5f6576656e7473436f756e74"
);
atom!(
    S55,
    A55,
    b"_maxListeners",
    b"_maxListeners\0",
    "perry_image_literal_0_5f6d61784c697374656e657273",
    "perry_atom_5f6d61784c697374656e657273"
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

static READABLE_STATE_PROTO_KEYS: [PerryLayoutKey; 16] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A0),
        slot: 0,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A1),
        slot: 1,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A2),
        slot: 2,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A3),
        slot: 3,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A4),
        slot: 4,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A5),
        slot: 5,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A6),
        slot: 6,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A7),
        slot: 7,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A8),
        slot: 8,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A9),
        slot: 9,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A10),
        slot: 10,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A11),
        slot: 11,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A12),
        slot: 12,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A13),
        slot: 13,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A14),
        slot: 14,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A15),
        slot: 15,
        attrs: 58,
        _pad: [0; 3],
    },
];
static READABLE_STATE_PROTO_INDEX: [u32; 32] = [
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
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut READABLE_STATE_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500001,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 16,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: READABLE_STATE_PROTO_KEYS.as_ptr(),
    key_count: 16,
    index_mask: 31,
    index: READABLE_STATE_PROTO_INDEX.as_ptr(),
};
static WRITABLE_STATE_PROTO_KEYS: [PerryLayoutKey; 16] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A0),
        slot: 0,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A16),
        slot: 1,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A17),
        slot: 2,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A18),
        slot: 3,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A3),
        slot: 4,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A19),
        slot: 5,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A10),
        slot: 6,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A20),
        slot: 7,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A7),
        slot: 8,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A8),
        slot: 9,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A9),
        slot: 10,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A11),
        slot: 11,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A12),
        slot: 12,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A14),
        slot: 13,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A21),
        slot: 14,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A22),
        slot: 15,
        attrs: 58,
        _pad: [0; 3],
    },
];
static WRITABLE_STATE_PROTO_INDEX: [u32; 32] = [
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
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut WRITABLE_STATE_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500002,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 16,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: WRITABLE_STATE_PROTO_KEYS.as_ptr(),
    key_count: 16,
    index_mask: 31,
    index: WRITABLE_STATE_PROTO_INDEX.as_ptr(),
};
static DUPLEX_PROTO_KEYS: [PerryLayoutKey; 17] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A23),
        slot: 0,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A24),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A25),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A26),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A27),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A28),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A29),
        slot: 6,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A10),
        slot: 7,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A11),
        slot: 8,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A30),
        slot: 9,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A31),
        slot: 10,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A32),
        slot: 11,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A33),
        slot: 12,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A34),
        slot: 13,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A22),
        slot: 14,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A35),
        slot: 15,
        attrs: 58,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A36),
        slot: 16,
        attrs: 58,
        _pad: [0; 3],
    },
];
static DUPLEX_PROTO_INDEX: [u32; 64] = [
    u32::MAX,
    13,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    5,
    8,
    u32::MAX,
    u32::MAX,
    11,
    2,
    u32::MAX,
    7,
    u32::MAX,
    u32::MAX,
    0,
    u32::MAX,
    14,
    u32::MAX,
    4,
    u32::MAX,
    9,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    6,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    3,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    10,
    12,
    15,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    1,
    u32::MAX,
    u32::MAX,
    16,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut DUPLEX_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500003,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 17,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: DUPLEX_PROTO_KEYS.as_ptr(),
    key_count: 17,
    index_mask: 63,
    index: DUPLEX_PROTO_INDEX.as_ptr(),
};
static TRANSFORM_PROTO_KEYS: [PerryLayoutKey; 2] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A23),
        slot: 0,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A37),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
];
static TRANSFORM_PROTO_INDEX: [u32; 4] = [u32::MAX, u32::MAX, 1, 0];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut TRANSFORM_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500004,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 2,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: TRANSFORM_PROTO_KEYS.as_ptr(),
    key_count: 2,
    index_mask: 3,
    index: TRANSFORM_PROTO_INDEX.as_ptr(),
};
static PASSTHROUGH_PROTO_KEYS: [PerryLayoutKey; 1] = [PerryLayoutKey {
    atom: std::ptr::addr_of!(A23),
    slot: 0,
    attrs: 2,
    _pad: [0; 3],
}];
static PASSTHROUGH_PROTO_INDEX: [u32; 2] = [u32::MAX, 0];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut PASSTHROUGH_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500005,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 1,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: PASSTHROUGH_PROTO_KEYS.as_ptr(),
    key_count: 1,
    index_mask: 1,
    index: PASSTHROUGH_PROTO_INDEX.as_ptr(),
};
static LEGACY_STREAM_PROTO_KEYS: [PerryLayoutKey; 16] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A23),
        slot: 0,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A38),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A39),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A40),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A41),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A42),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A43),
        slot: 6,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A44),
        slot: 7,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A45),
        slot: 8,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A46),
        slot: 9,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A47),
        slot: 10,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A48),
        slot: 11,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A49),
        slot: 12,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A50),
        slot: 13,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A51),
        slot: 14,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A52),
        slot: 15,
        attrs: 0,
        _pad: [0; 3],
    },
];
static LEGACY_STREAM_PROTO_INDEX: [u32; 32] = [
    10,
    u32::MAX,
    15,
    u32::MAX,
    2,
    6,
    4,
    14,
    13,
    u32::MAX,
    5,
    9,
    u32::MAX,
    8,
    u32::MAX,
    0,
    1,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    11,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    7,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    12,
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
pub(crate) static mut LEGACY_STREAM_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500006,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 16,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: LEGACY_STREAM_PROTO_KEYS.as_ptr(),
    key_count: 16,
    index_mask: 31,
    index: LEGACY_STREAM_PROTO_INDEX.as_ptr(),
};
static EVENT_EMITTER_PROTO_KEYS: [PerryLayoutKey; 19] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A23),
        slot: 0,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A53),
        slot: 1,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A54),
        slot: 2,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A55),
        slot: 3,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A47),
        slot: 4,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A48),
        slot: 5,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A46),
        slot: 6,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A43),
        slot: 7,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A38),
        slot: 8,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A40),
        slot: 9,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A39),
        slot: 10,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A41),
        slot: 11,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A44),
        slot: 12,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A42),
        slot: 13,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A45),
        slot: 14,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A51),
        slot: 15,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A52),
        slot: 16,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A50),
        slot: 17,
        attrs: 0,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A49),
        slot: 18,
        attrs: 0,
        _pad: [0; 3],
    },
];
static EVENT_EMITTER_PROTO_INDEX: [u32; 64] = [
    u32::MAX,
    3,
    u32::MAX,
    u32::MAX,
    10,
    u32::MAX,
    15,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    6,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    0,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    5,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    12,
    2,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    4,
    u32::MAX,
    16,
    u32::MAX,
    7,
    u32::MAX,
    11,
    u32::MAX,
    17,
    u32::MAX,
    13,
    1,
    u32::MAX,
    14,
    u32::MAX,
    u32::MAX,
    8,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    u32::MAX,
    18,
    u32::MAX,
    9,
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut EVENT_EMITTER_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500007,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 19,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: EVENT_EMITTER_PROTO_KEYS.as_ptr(),
    key_count: 19,
    index_mask: 63,
    index: EVENT_EMITTER_PROTO_INDEX.as_ptr(),
};
static EE_ASYNC_RESOURCE_PROTO_KEYS: [PerryLayoutKey; 6] = [
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A23),
        slot: 0,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A46),
        slot: 1,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A56),
        slot: 2,
        attrs: 2,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A57),
        slot: 3,
        attrs: 26,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A58),
        slot: 4,
        attrs: 26,
        _pad: [0; 3],
    },
    PerryLayoutKey {
        atom: std::ptr::addr_of!(A59),
        slot: 5,
        attrs: 26,
        _pad: [0; 3],
    },
];
static EE_ASYNC_RESOURCE_PROTO_INDEX: [u32; 16] = [
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
];
#[used(linker)]
#[cfg_attr(target_vendor = "apple", link_section = "__DATA,__perry_layouts")]
#[cfg_attr(target_os = "windows", link_section = ".perrylay$m")]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "windows")),
    link_section = "perry_layouts"
)]
pub(crate) static mut EE_ASYNC_RESOURCE_PROTO: PerryLayoutDesc = PerryLayoutDesc {
    family: 0x53500008,
    variant: 0,
    kind: 0,
    integrity: 0,
    slot_count: 6,
    layout_id: super::super::layout_desc::LAYOUT_ID_CARRIED,
    rep: 0,
    proto: crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
    keys: EE_ASYNC_RESOURCE_PROTO_KEYS.as_ptr(),
    key_count: 6,
    index_mask: 15,
    index: EE_ASYNC_RESOURCE_PROTO_INDEX.as_ptr(),
};
