//! The global object's builtin names as image atoms (DEC 243(5)). Each is the
//! image string and static atom a program naming the same key links to (the
//! export names codegen gives them), so the agent's atom for a builtin global
//! name is image data: the records are not in the atom section (no startup
//! seed); the atom table answers them at a miss from the `GLOBAL_OBJECT`
//! layout index. Populate keys the global object with them and allocates no
//! name, and every key of the global object is an atom.
use super::intrinsic_private_atoms::REGEXP_PROTOTYPE;
use super::*;

macro_rules! global_atom {
    ($string:ident, $atom:ident, $name:literal, $image_export:literal, $atom_export:literal) => {
        #[used(linker)]
        #[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
        #[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
        #[cfg_attr(
            not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
            link_section = "perry_strings"
        )]
        #[export_name = $image_export]
        static $string: ImageString<{ $name.len() + 1 }> =
            image_bytes(concat!($name, "\0").as_bytes());
        wasm_string_image!($string);
        // Not in the atom section: the static seed never walks it. The
        // GLOBAL_OBJECT layout index (compile-time placed) is where the
        // agent's atom table finds it (`builtin_global_atom` at a miss).
        #[used(linker)]
        #[export_name = $atom_export]
        pub(crate) static $atom: StaticAtom = StaticAtom {
            hash: atom_hash($name.as_bytes()),
            string: std::ptr::addr_of!($string.string),
            kind: crate::codegen_abi::ATOM_KIND_STRING,
            _pad: [0; 3],
            index: 0,
        };
        const _: () = assert!(export_spells($atom_export.as_bytes(), $name.as_bytes()));
    };
}

/// `export` is `perry_atom_` and the lower-case hex of `name`'s bytes: the
/// name codegen gives the atom of that text.
const fn export_spells(export: &[u8], name: &[u8]) -> bool {
    const PREFIX: &[u8] = b"perry_atom_";
    const HEX: &[u8] = b"0123456789abcdef";
    if export.len() != PREFIX.len() + 2 * name.len() {
        return false;
    }
    let mut i = 0;
    while i < PREFIX.len() {
        if export[i] != PREFIX[i] {
            return false;
        }
        i += 1;
    }
    let mut j = 0;
    while j < name.len() {
        let at = PREFIX.len() + 2 * j;
        if export[at] != HEX[(name[j] >> 4) as usize]
            || export[at + 1] != HEX[(name[j] & 15) as usize]
        {
            return false;
        }
        j += 1;
    }
    true
}

global_atom!(
    G0,
    GA0,
    "globalThis",
    "perry_image_literal_0_676c6f62616c54686973",
    "perry_atom_676c6f62616c54686973"
);
global_atom!(
    G1,
    GA1,
    "global",
    "perry_image_literal_0_676c6f62616c",
    "perry_atom_676c6f62616c"
);
global_atom!(
    G2,
    GA2,
    "Array",
    "perry_image_literal_0_4172726179",
    "perry_atom_4172726179"
);
global_atom!(
    G3,
    GA3,
    "Object",
    "perry_image_literal_0_4f626a656374",
    "perry_atom_4f626a656374"
);
global_atom!(
    G4,
    GA4,
    "String",
    "perry_image_literal_0_537472696e67",
    "perry_atom_537472696e67"
);
global_atom!(
    G5,
    GA5,
    "Number",
    "perry_image_literal_0_4e756d626572",
    "perry_atom_4e756d626572"
);
global_atom!(
    G6,
    GA6,
    "Boolean",
    "perry_image_literal_0_426f6f6c65616e",
    "perry_atom_426f6f6c65616e"
);
global_atom!(
    G7,
    GA7,
    "Function",
    "perry_image_literal_0_46756e6374696f6e",
    "perry_atom_46756e6374696f6e"
);
global_atom!(
    G8,
    GA8,
    "RegExp",
    "perry_image_literal_0_526567457870",
    "perry_atom_526567457870"
);
global_atom!(
    G9,
    GA9,
    "Date",
    "perry_image_literal_0_44617465",
    "perry_atom_44617465"
);
global_atom!(
    G10,
    GA10,
    "Error",
    "perry_image_literal_0_4572726f72",
    "perry_atom_4572726f72"
);
global_atom!(
    G11,
    GA11,
    "TypeError",
    "perry_image_literal_0_547970654572726f72",
    "perry_atom_547970654572726f72"
);
global_atom!(
    G12,
    GA12,
    "RangeError",
    "perry_image_literal_0_52616e67654572726f72",
    "perry_atom_52616e67654572726f72"
);
global_atom!(
    G13,
    GA13,
    "SyntaxError",
    "perry_image_literal_0_53796e7461784572726f72",
    "perry_atom_53796e7461784572726f72"
);
global_atom!(
    G14,
    GA14,
    "ReferenceError",
    "perry_image_literal_0_5265666572656e63654572726f72",
    "perry_atom_5265666572656e63654572726f72"
);
global_atom!(
    G15,
    GA15,
    "EvalError",
    "perry_image_literal_0_4576616c4572726f72",
    "perry_atom_4576616c4572726f72"
);
global_atom!(
    G16,
    GA16,
    "URIError",
    "perry_image_literal_0_5552494572726f72",
    "perry_atom_5552494572726f72"
);
global_atom!(
    G17,
    GA17,
    "AggregateError",
    "perry_image_literal_0_4167677265676174654572726f72",
    "perry_atom_4167677265676174654572726f72"
);
global_atom!(
    G18,
    GA18,
    "Symbol",
    "perry_image_literal_0_53796d626f6c",
    "perry_atom_53796d626f6c"
);
global_atom!(
    G19,
    GA19,
    "Promise",
    "perry_image_literal_0_50726f6d697365",
    "perry_atom_50726f6d697365"
);
global_atom!(
    G20,
    GA20,
    "Map",
    "perry_image_literal_0_4d6170",
    "perry_atom_4d6170"
);
global_atom!(
    G21,
    GA21,
    "Set",
    "perry_image_literal_0_536574",
    "perry_atom_536574"
);
global_atom!(
    G22,
    GA22,
    "WeakMap",
    "perry_image_literal_0_5765616b4d6170",
    "perry_atom_5765616b4d6170"
);
global_atom!(
    G23,
    GA23,
    "WeakSet",
    "perry_image_literal_0_5765616b536574",
    "perry_atom_5765616b536574"
);
global_atom!(
    G24,
    GA24,
    "WeakRef",
    "perry_image_literal_0_5765616b526566",
    "perry_atom_5765616b526566"
);
global_atom!(
    G25,
    GA25,
    "Proxy",
    "perry_image_literal_0_50726f7879",
    "perry_atom_50726f7879"
);
global_atom!(
    G26,
    GA26,
    "BigInt",
    "perry_image_literal_0_426967496e74",
    "perry_atom_426967496e74"
);
global_atom!(
    G27,
    GA27,
    "Uint8Array",
    "perry_image_literal_0_55696e74384172726179",
    "perry_atom_55696e74384172726179"
);
global_atom!(
    G28,
    GA28,
    "Int8Array",
    "perry_image_literal_0_496e74384172726179",
    "perry_atom_496e74384172726179"
);
global_atom!(
    G29,
    GA29,
    "Uint16Array",
    "perry_image_literal_0_55696e7431364172726179",
    "perry_atom_55696e7431364172726179"
);
global_atom!(
    G30,
    GA30,
    "Int16Array",
    "perry_image_literal_0_496e7431364172726179",
    "perry_atom_496e7431364172726179"
);
global_atom!(
    G31,
    GA31,
    "Uint32Array",
    "perry_image_literal_0_55696e7433324172726179",
    "perry_atom_55696e7433324172726179"
);
global_atom!(
    G32,
    GA32,
    "Int32Array",
    "perry_image_literal_0_496e7433324172726179",
    "perry_atom_496e7433324172726179"
);
global_atom!(
    G33,
    GA33,
    "Float16Array",
    "perry_image_literal_0_466c6f617431364172726179",
    "perry_atom_466c6f617431364172726179"
);
global_atom!(
    G34,
    GA34,
    "Float32Array",
    "perry_image_literal_0_466c6f617433324172726179",
    "perry_atom_466c6f617433324172726179"
);
global_atom!(
    G35,
    GA35,
    "Float64Array",
    "perry_image_literal_0_466c6f617436344172726179",
    "perry_atom_466c6f617436344172726179"
);
global_atom!(
    G36,
    GA36,
    "Uint8ClampedArray",
    "perry_image_literal_0_55696e7438436c616d7065644172726179",
    "perry_atom_55696e7438436c616d7065644172726179"
);
global_atom!(
    G37,
    GA37,
    "BigInt64Array",
    "perry_image_literal_0_426967496e7436344172726179",
    "perry_atom_426967496e7436344172726179"
);
global_atom!(
    G38,
    GA38,
    "BigUint64Array",
    "perry_image_literal_0_42696755696e7436344172726179",
    "perry_atom_42696755696e7436344172726179"
);
global_atom!(
    G39,
    GA39,
    "ArrayBuffer",
    "perry_image_literal_0_4172726179427566666572",
    "perry_atom_4172726179427566666572"
);
global_atom!(
    G40,
    GA40,
    "SharedArrayBuffer",
    "perry_image_literal_0_5368617265644172726179427566666572",
    "perry_atom_5368617265644172726179427566666572"
);
global_atom!(
    G41,
    GA41,
    "DataView",
    "perry_image_literal_0_4461746156696577",
    "perry_atom_4461746156696577"
);
global_atom!(
    G42,
    GA42,
    "TextEncoder",
    "perry_image_literal_0_54657874456e636f646572",
    "perry_atom_54657874456e636f646572"
);
global_atom!(
    G43,
    GA43,
    "TextDecoder",
    "perry_image_literal_0_546578744465636f646572",
    "perry_atom_546578744465636f646572"
);
global_atom!(
    G44,
    GA44,
    "TextEncoderStream",
    "perry_image_literal_0_54657874456e636f64657253747265616d",
    "perry_atom_54657874456e636f64657253747265616d"
);
global_atom!(
    G45,
    GA45,
    "TextDecoderStream",
    "perry_image_literal_0_546578744465636f64657253747265616d",
    "perry_atom_546578744465636f64657253747265616d"
);
global_atom!(
    G46,
    GA46,
    "CompressionStream",
    "perry_image_literal_0_436f6d7072657373696f6e53747265616d",
    "perry_atom_436f6d7072657373696f6e53747265616d"
);
global_atom!(
    G47,
    GA47,
    "DecompressionStream",
    "perry_image_literal_0_4465636f6d7072657373696f6e53747265616d",
    "perry_atom_4465636f6d7072657373696f6e53747265616d"
);
global_atom!(
    G48,
    GA48,
    "ReadableStream",
    "perry_image_literal_0_5265616461626c6553747265616d",
    "perry_atom_5265616461626c6553747265616d"
);
global_atom!(
    G49,
    GA49,
    "WritableStream",
    "perry_image_literal_0_5772697461626c6553747265616d",
    "perry_atom_5772697461626c6553747265616d"
);
global_atom!(
    G50,
    GA50,
    "TransformStream",
    "perry_image_literal_0_5472616e73666f726d53747265616d",
    "perry_atom_5472616e73666f726d53747265616d"
);
global_atom!(
    G51,
    GA51,
    "Navigator",
    "perry_image_literal_0_4e6176696761746f72",
    "perry_atom_4e6176696761746f72"
);
global_atom!(
    G52,
    GA52,
    "URL",
    "perry_image_literal_0_55524c",
    "perry_atom_55524c"
);
global_atom!(
    G53,
    GA53,
    "URLSearchParams",
    "perry_image_literal_0_55524c536561726368506172616d73",
    "perry_atom_55524c536561726368506172616d73"
);
global_atom!(
    G54,
    GA54,
    "URLPattern",
    "perry_image_literal_0_55524c5061747465726e",
    "perry_atom_55524c5061747465726e"
);
global_atom!(
    G55,
    GA55,
    "AbortController",
    "perry_image_literal_0_41626f7274436f6e74726f6c6c6572",
    "perry_atom_41626f7274436f6e74726f6c6c6572"
);
global_atom!(
    G56,
    GA56,
    "AbortSignal",
    "perry_image_literal_0_41626f72745369676e616c",
    "perry_atom_41626f72745369676e616c"
);
global_atom!(
    G57,
    GA57,
    "EventTarget",
    "perry_image_literal_0_4576656e74546172676574",
    "perry_atom_4576656e74546172676574"
);
global_atom!(
    G58,
    GA58,
    "Crypto",
    "perry_image_literal_0_43727970746f",
    "perry_atom_43727970746f"
);
global_atom!(
    G59,
    GA59,
    "CryptoKey",
    "perry_image_literal_0_43727970746f4b6579",
    "perry_atom_43727970746f4b6579"
);
global_atom!(
    G60,
    GA60,
    "SubtleCrypto",
    "perry_image_literal_0_537562746c6543727970746f",
    "perry_atom_537562746c6543727970746f"
);
global_atom!(
    G61,
    GA61,
    "Event",
    "perry_image_literal_0_4576656e74",
    "perry_atom_4576656e74"
);
global_atom!(
    G62,
    GA62,
    "CustomEvent",
    "perry_image_literal_0_437573746f6d4576656e74",
    "perry_atom_437573746f6d4576656e74"
);
global_atom!(
    G63,
    GA63,
    "DOMException",
    "perry_image_literal_0_444f4d457863657074696f6e",
    "perry_atom_444f4d457863657074696f6e"
);
global_atom!(
    G64,
    GA64,
    "FormData",
    "perry_image_literal_0_466f726d44617461",
    "perry_atom_466f726d44617461"
);
global_atom!(
    G65,
    GA65,
    "Blob",
    "perry_image_literal_0_426c6f62",
    "perry_atom_426c6f62"
);
global_atom!(
    G66,
    GA66,
    "File",
    "perry_image_literal_0_46696c65",
    "perry_atom_46696c65"
);
global_atom!(
    G67,
    GA67,
    "Headers",
    "perry_image_literal_0_48656164657273",
    "perry_atom_48656164657273"
);
global_atom!(
    G68,
    GA68,
    "Request",
    "perry_image_literal_0_52657175657374",
    "perry_atom_52657175657374"
);
global_atom!(
    G69,
    GA69,
    "Response",
    "perry_image_literal_0_526573706f6e7365",
    "perry_atom_526573706f6e7365"
);
global_atom!(
    G70,
    GA70,
    "MessageChannel",
    "perry_image_literal_0_4d6573736167654368616e6e656c",
    "perry_atom_4d6573736167654368616e6e656c"
);
global_atom!(
    G71,
    GA71,
    "MessagePort",
    "perry_image_literal_0_4d657373616765506f7274",
    "perry_atom_4d657373616765506f7274"
);
global_atom!(
    G72,
    GA72,
    "BroadcastChannel",
    "perry_image_literal_0_42726f6164636173744368616e6e656c",
    "perry_atom_42726f6164636173744368616e6e656c"
);
global_atom!(
    G73,
    GA73,
    "Storage",
    "perry_image_literal_0_53746f72616765",
    "perry_atom_53746f72616765"
);
global_atom!(
    G74,
    GA74,
    "localStorage",
    "perry_image_literal_0_6c6f63616c53746f72616765",
    "perry_atom_6c6f63616c53746f72616765"
);
global_atom!(
    G75,
    GA75,
    "sessionStorage",
    "perry_image_literal_0_73657373696f6e53746f72616765",
    "perry_atom_73657373696f6e53746f72616765"
);
global_atom!(
    G76,
    GA76,
    "WebSocket",
    "perry_image_literal_0_576562536f636b6574",
    "perry_atom_576562536f636b6574"
);
global_atom!(
    G77,
    GA77,
    "FinalizationRegistry",
    "perry_image_literal_0_46696e616c697a6174696f6e5265676973747279",
    "perry_atom_46696e616c697a6174696f6e5265676973747279"
);
global_atom!(
    G78,
    GA78,
    "DisposableStack",
    "perry_image_literal_0_446973706f7361626c65537461636b",
    "perry_atom_446973706f7361626c65537461636b"
);
global_atom!(
    G79,
    GA79,
    "AsyncDisposableStack",
    "perry_image_literal_0_4173796e63446973706f7361626c65537461636b",
    "perry_atom_4173796e63446973706f7361626c65537461636b"
);
global_atom!(
    G80,
    GA80,
    "SuppressedError",
    "perry_image_literal_0_537570707265737365644572726f72",
    "perry_atom_537570707265737365644572726f72"
);
global_atom!(
    G81,
    GA81,
    "Iterator",
    "perry_image_literal_0_4974657261746f72",
    "perry_atom_4974657261746f72"
);
global_atom!(
    G82,
    GA82,
    "Buffer",
    "perry_image_literal_0_427566666572",
    "perry_atom_427566666572"
);
global_atom!(
    G83,
    GA83,
    "eval",
    "perry_image_literal_0_6576616c",
    "perry_atom_6576616c"
);
global_atom!(
    G84,
    GA84,
    "fetch",
    "perry_image_literal_0_6665746368",
    "perry_atom_6665746368"
);
global_atom!(
    G85,
    GA85,
    "structuredClone",
    "perry_image_literal_0_73747275637475726564436c6f6e65",
    "perry_atom_73747275637475726564436c6f6e65"
);
global_atom!(
    G86,
    GA86,
    "atob",
    "perry_image_literal_0_61746f62",
    "perry_atom_61746f62"
);
global_atom!(
    G87,
    GA87,
    "btoa",
    "perry_image_literal_0_62746f61",
    "perry_atom_62746f61"
);
global_atom!(
    G88,
    GA88,
    "clearTimeout",
    "perry_image_literal_0_636c65617254696d656f7574",
    "perry_atom_636c65617254696d656f7574"
);
global_atom!(
    G89,
    GA89,
    "setInterval",
    "perry_image_literal_0_736574496e74657276616c",
    "perry_atom_736574496e74657276616c"
);
global_atom!(
    G90,
    GA90,
    "clearInterval",
    "perry_image_literal_0_636c656172496e74657276616c",
    "perry_atom_636c656172496e74657276616c"
);
global_atom!(
    G91,
    GA91,
    "setImmediate",
    "perry_image_literal_0_736574496d6d656469617465",
    "perry_atom_736574496d6d656469617465"
);
global_atom!(
    G92,
    GA92,
    "clearImmediate",
    "perry_image_literal_0_636c656172496d6d656469617465",
    "perry_atom_636c656172496d6d656469617465"
);
global_atom!(
    G93,
    GA93,
    "queueMicrotask",
    "perry_image_literal_0_71756575654d6963726f7461736b",
    "perry_atom_71756575654d6963726f7461736b"
);
global_atom!(
    G94,
    GA94,
    "gc",
    "perry_image_literal_0_6763",
    "perry_atom_6763"
);
global_atom!(
    G95,
    GA95,
    "parseInt",
    "perry_image_literal_0_7061727365496e74",
    "perry_atom_7061727365496e74"
);
global_atom!(
    G96,
    GA96,
    "parseFloat",
    "perry_image_literal_0_7061727365466c6f6174",
    "perry_atom_7061727365466c6f6174"
);
global_atom!(
    G97,
    GA97,
    "isNaN",
    "perry_image_literal_0_69734e614e",
    "perry_atom_69734e614e"
);
global_atom!(
    G98,
    GA98,
    "isFinite",
    "perry_image_literal_0_697346696e697465",
    "perry_atom_697346696e697465"
);
global_atom!(
    G99,
    GA99,
    "encodeURI",
    "perry_image_literal_0_656e636f6465555249",
    "perry_atom_656e636f6465555249"
);
global_atom!(
    G100,
    GA100,
    "decodeURI",
    "perry_image_literal_0_6465636f6465555249",
    "perry_atom_6465636f6465555249"
);
global_atom!(
    G101,
    GA101,
    "encodeURIComponent",
    "perry_image_literal_0_656e636f6465555249436f6d706f6e656e74",
    "perry_atom_656e636f6465555249436f6d706f6e656e74"
);
global_atom!(
    G102,
    GA102,
    "decodeURIComponent",
    "perry_image_literal_0_6465636f6465555249436f6d706f6e656e74",
    "perry_atom_6465636f6465555249436f6d706f6e656e74"
);
global_atom!(
    G103,
    GA103,
    "escape",
    "perry_image_literal_0_657363617065",
    "perry_atom_657363617065"
);
global_atom!(
    G104,
    GA104,
    "unescape",
    "perry_image_literal_0_756e657363617065",
    "perry_atom_756e657363617065"
);
global_atom!(
    G105,
    GA105,
    "console",
    "perry_image_literal_0_636f6e736f6c65",
    "perry_atom_636f6e736f6c65"
);
global_atom!(
    G106,
    GA106,
    "process",
    "perry_image_literal_0_70726f63657373",
    "perry_atom_70726f63657373"
);
global_atom!(
    G107,
    GA107,
    "Math",
    "perry_image_literal_0_4d617468",
    "perry_atom_4d617468"
);
global_atom!(
    G108,
    GA108,
    "JSON",
    "perry_image_literal_0_4a534f4e",
    "perry_atom_4a534f4e"
);
global_atom!(
    G109,
    GA109,
    "Reflect",
    "perry_image_literal_0_5265666c656374",
    "perry_atom_5265666c656374"
);
global_atom!(
    G110,
    GA110,
    "Atomics",
    "perry_image_literal_0_41746f6d696373",
    "perry_atom_41746f6d696373"
);
global_atom!(
    G111,
    GA111,
    "Intl",
    "perry_image_literal_0_496e746c",
    "perry_atom_496e746c"
);
global_atom!(
    G112,
    GA112,
    "WebAssembly",
    "perry_image_literal_0_576562417373656d626c79",
    "perry_atom_576562417373656d626c79"
);
global_atom!(
    G113,
    GA113,
    "Temporal",
    "perry_image_literal_0_54656d706f72616c",
    "perry_atom_54656d706f72616c"
);
global_atom!(
    G114,
    GA114,
    "performance",
    "perry_image_literal_0_706572666f726d616e6365",
    "perry_atom_706572666f726d616e6365"
);
global_atom!(
    G115,
    GA115,
    "Performance",
    "perry_image_literal_0_506572666f726d616e6365",
    "perry_atom_506572666f726d616e6365"
);
global_atom!(
    G116,
    GA116,
    "PerformanceEntry",
    "perry_image_literal_0_506572666f726d616e6365456e747279",
    "perry_atom_506572666f726d616e6365456e747279"
);
global_atom!(
    G117,
    GA117,
    "PerformanceMark",
    "perry_image_literal_0_506572666f726d616e63654d61726b",
    "perry_atom_506572666f726d616e63654d61726b"
);
global_atom!(
    G118,
    GA118,
    "PerformanceMeasure",
    "perry_image_literal_0_506572666f726d616e63654d656173757265",
    "perry_atom_506572666f726d616e63654d656173757265"
);
global_atom!(
    G119,
    GA119,
    "PerformanceObserver",
    "perry_image_literal_0_506572666f726d616e63654f62736572766572",
    "perry_atom_506572666f726d616e63654f62736572766572"
);
global_atom!(
    G120,
    GA120,
    "PerformanceObserverEntryList",
    "perry_image_literal_0_506572666f726d616e63654f62736572766572456e7472794c697374",
    "perry_atom_506572666f726d616e63654f62736572766572456e7472794c697374"
);
global_atom!(
    G121,
    GA121,
    "PerformanceResourceTiming",
    "perry_image_literal_0_506572666f726d616e63655265736f7572636554696d696e67",
    "perry_atom_506572666f726d616e63655265736f7572636554696d696e67"
);
global_atom!(
    G122,
    GA122,
    "crypto",
    "perry_image_literal_0_63727970746f",
    "perry_atom_63727970746f"
);
global_atom!(
    G123,
    GA123,
    "navigator",
    "perry_image_literal_0_6e6176696761746f72",
    "perry_atom_6e6176696761746f72"
);
global_atom!(
    G124,
    GA124,
    "NaN",
    "perry_image_literal_0_4e614e",
    "perry_atom_4e614e"
);
global_atom!(
    G125,
    GA125,
    "Infinity",
    "perry_image_literal_0_496e66696e697479",
    "perry_atom_496e66696e697479"
);
global_atom!(
    G126,
    GA126,
    "undefined",
    "perry_image_literal_0_756e646566696e6564",
    "perry_atom_756e646566696e6564"
);
global_atom!(
    G127,
    GA127,
    "MessageEvent",
    "perry_image_literal_0_4d6573736167654576656e74",
    "perry_atom_4d6573736167654576656e74"
);
global_atom!(
    G128,
    GA128,
    "ReadableStreamDefaultController",
    "perry_image_literal_0_5265616461626c6553747265616d44656661756c74436f6e74726f6c6c6572",
    "perry_atom_5265616461626c6553747265616d44656661756c74436f6e74726f6c6c6572"
);
global_atom!(
    G129,
    GA129,
    "ReadableStreamDefaultReader",
    "perry_image_literal_0_5265616461626c6553747265616d44656661756c74526561646572",
    "perry_atom_5265616461626c6553747265616d44656661756c74526561646572"
);
global_atom!(
    G130,
    GA130,
    "ReadableByteStreamController",
    "perry_image_literal_0_5265616461626c654279746553747265616d436f6e74726f6c6c6572",
    "perry_atom_5265616461626c654279746553747265616d436f6e74726f6c6c6572"
);
global_atom!(
    G131,
    GA131,
    "ReadableStreamBYOBReader",
    "perry_image_literal_0_5265616461626c6553747265616d42594f42526561646572",
    "perry_atom_5265616461626c6553747265616d42594f42526561646572"
);
global_atom!(
    G132,
    GA132,
    "ReadableStreamBYOBRequest",
    "perry_image_literal_0_5265616461626c6553747265616d42594f4252657175657374",
    "perry_atom_5265616461626c6553747265616d42594f4252657175657374"
);

// The global object's own builtin keys in their enumeration (populate) order,
// each with its attribute byte: 2 = writable, configurable (not enumerable);
// 0 = writable, enumerable, configurable; 7 = read-only, not enumerable, not
// configurable (NaN, Infinity, undefined); 57/59 = the sessionStorage /
// localStorage accessors (getter and setter) as populate defines them; 24 =
// enumerable getter only. The last key is the realm's private
// `[[RegExpPrototype]]` slot (66 = private, not enumerable), which populate
// stores with the RegExp intrinsic. The global object is born on this layout
// (`js_get_global_this`) in one publish, and population fills its slots.
runtime_layout!(GLOBAL_OBJECT, 0x5254_0012, 0, crate::object::layout_desc::LAYOUT_PROTO_OBJECT, [
    GA0 => 2,
    GA1 => 0,
    GA2 => 2,
    GA3 => 2,
    GA4 => 2,
    GA5 => 2,
    GA6 => 2,
    GA7 => 2,
    GA8 => 2,
    GA9 => 2,
    GA10 => 2,
    GA11 => 2,
    GA12 => 2,
    GA13 => 2,
    GA14 => 2,
    GA15 => 2,
    GA16 => 2,
    GA17 => 2,
    GA18 => 2,
    GA19 => 2,
    GA20 => 2,
    GA21 => 2,
    GA22 => 2,
    GA23 => 2,
    GA24 => 2,
    GA25 => 2,
    GA26 => 2,
    GA27 => 2,
    GA28 => 2,
    GA29 => 2,
    GA30 => 2,
    GA31 => 2,
    GA32 => 2,
    GA33 => 2,
    GA34 => 2,
    GA35 => 2,
    GA36 => 2,
    GA37 => 2,
    GA38 => 2,
    GA39 => 2,
    GA40 => 2,
    GA41 => 2,
    GA42 => 2,
    GA43 => 2,
    GA44 => 2,
    GA45 => 2,
    GA46 => 2,
    GA47 => 2,
    GA48 => 2,
    GA128 => 2,
    GA129 => 2,
    GA130 => 2,
    GA131 => 2,
    GA132 => 2,
    GA49 => 2,
    GA50 => 2,
    GA51 => 2,
    GA52 => 2,
    GA53 => 2,
    GA54 => 2,
    GA55 => 2,
    GA56 => 2,
    GA57 => 2,
    GA58 => 2,
    GA59 => 2,
    GA60 => 2,
    GA61 => 2,
    GA62 => 2,
    GA127 => 2,
    GA63 => 2,
    GA64 => 2,
    GA65 => 2,
    GA66 => 2,
    GA67 => 2,
    GA68 => 2,
    GA69 => 2,
    GA70 => 2,
    GA71 => 2,
    GA72 => 2,
    GA73 => 2,
    GA74 => 59,
    GA75 => 57,
    GA76 => 2,
    GA77 => 2,
    GA78 => 2,
    GA79 => 2,
    GA80 => 2,
    GA81 => 2,
    GA82 => 2,
    GA83 => 2,
    GA84 => 0,
    GA85 => 0,
    GA86 => 0,
    GA87 => 0,
    NP_ATOM_SET_TIMEOUT => 0,
    GA88 => 0,
    GA89 => 0,
    GA90 => 0,
    GA91 => 0,
    GA92 => 0,
    GA93 => 0,
    GA94 => 2,
    GA95 => 2,
    GA96 => 2,
    GA97 => 2,
    GA98 => 2,
    GA99 => 2,
    GA100 => 2,
    GA101 => 2,
    GA102 => 2,
    GA103 => 2,
    GA104 => 2,
    GA105 => 2,
    GA106 => 2,
    GA107 => 2,
    GA108 => 2,
    GA109 => 2,
    GA110 => 2,
    GA111 => 2,
    GA112 => 2,
    GA113 => 2,
    GA114 => 0,
    GA115 => 0,
    GA116 => 0,
    GA117 => 0,
    GA118 => 0,
    GA119 => 0,
    GA120 => 0,
    GA121 => 0,
    GA122 => 24,
    GA123 => 24,
    GA124 => 7,
    GA125 => 7,
    GA126 => 7,
    REGEXP_PROTOTYPE => 66,
], realm = ["%Global%"]);

/// The global object's own key named `name` (FNV-1a `hash`), found through
/// the index of its layout descriptor (the program's definition; `desc()`):
/// the builtin name's image atom.
#[inline(never)]
pub(crate) fn builtin_global_atom(name: &[u8], hash: u64) -> Option<*const StringHeader> {
    crate::realm_decl! { operation builtin_global_atom, demands = ["%Layout:GLOBAL_OBJECT%"] }
    // SAFETY: the descriptor is the program's immutable definition of the
    // layout; its keys and index are image data of that descriptor.
    let (keys, index, mask) = unsafe {
        let desc = GLOBAL_OBJECT::desc();
        ((*desc).keys, (*desc).index, (*desc).index_mask as usize)
    };
    let mut at = hash as usize & mask;
    loop {
        // SAFETY: `at <= mask`, the index capacity - 1.
        let pos = unsafe { *index.add(at) };
        if pos == u32::MAX {
            return None;
        }
        // SAFETY: an index entry is a position in `keys`.
        let atom = unsafe { (*keys.add(pos as usize)).atom };
        // SAFETY: the layout's atoms are this binary's static image atoms.
        unsafe {
            if (*atom).hash == hash {
                let string = (*atom).string;
                if (*string).byte_len as usize == name.len()
                    && std::slice::from_raw_parts(crate::string::string_data(string), name.len())
                        == name
                {
                    return Some(string);
                }
            }
        }
        at = (at + 1) & mask;
    }
}
