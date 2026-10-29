//! Exact descriptor facts preserved when the thirteen legacy declarations
//! moved onto runtime_layout!. This fixture is compiled only for tests.
use super::*;

#[test]
fn legacy_runtime_descriptor_facts_survive_single_source_migration() {
    unsafe {
        check(
            std::ptr::addr_of!(READABLE_STATE),
            "READABLE_STATE",
            0x52540001,
            crate::codegen_abi::native_layout_word("READABLE_STATE", 1),
            &raw const READABLE_STATE_PROTO_FAMILY,
            &[
                (ATOM_0.hash, 0, 0),
                (ATOM_1.hash, 1, 0),
                (ATOM_2.hash, 2, 0),
                (ATOM_3.hash, 3, 0),
                (ATOM_4.hash, 4, 0),
                (ATOM_5.hash, 5, 0),
            ],
            &[
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
            ],
        );
        check(
            std::ptr::addr_of!(WRITABLE_STATE),
            "WRITABLE_STATE",
            0x52540002,
            crate::codegen_abi::native_layout_word("WRITABLE_STATE", 1),
            &raw const WRITABLE_STATE_PROTO_FAMILY,
            &[
                (ATOM_0.hash, 0, 0),
                (ATOM_3.hash, 1, 0),
                (ATOM_6.hash, 2, 0),
                (ATOM_7.hash, 3, 0),
                (ATOM_8.hash, 4, 0),
                (ATOM_9.hash, 5, 0),
                (ATOM_10.hash, 6, 0),
            ],
            &[
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
            ],
        );
        check(
            std::ptr::addr_of!(URL_SEARCH_PARAMS),
            "URL_SEARCH_PARAMS",
            0x52540003,
            crate::codegen_abi::native_layout_word("URL_SEARCH_PARAMS", 1),
            &raw const URL_SEARCH_PARAMS_PROTO_FAMILY,
            &[(ATOM_11.hash, 0, 66), (ATOM_12.hash, 1, 66)],
            &[0, 1, u32::MAX, u32::MAX],
        );
        check(
            std::ptr::addr_of!(URL_COMPONENTS),
            "URL_COMPONENTS",
            0x52540004,
            crate::codegen_abi::native_layout_word("URL_COMPONENTS", 1),
            &raw const URL_PROTO_FAMILY,
            &[
                (ATOM_13.hash, 0, 66),
                (ATOM_14.hash, 1, 66),
                (ATOM_15.hash, 2, 66),
                (ATOM_16.hash, 3, 66),
                (ATOM_17.hash, 4, 66),
                (ATOM_18.hash, 5, 66),
                (ATOM_19.hash, 6, 66),
                (ATOM_20.hash, 7, 66),
                (ATOM_21.hash, 8, 66),
                (ATOM_22.hash, 9, 66),
                (ATOM_23.hash, 10, 66),
                (ATOM_24.hash, 11, 66),
            ],
            &[
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
            ],
        );
        check(
            std::ptr::addr_of!(ITERATOR_RESULT),
            "ITERATOR_RESULT",
            0x52540005,
            crate::codegen_abi::native_layout_word("ITERATOR_RESULT", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[(ATOM_25.hash, 0, 0), (ATOM_26.hash, 1, 0)],
            &[u32::MAX, 1, 0, u32::MAX],
        );
        check(
            std::ptr::addr_of!(ITERATOR_RESULT_DONE_FIRST),
            "ITERATOR_RESULT_DONE_FIRST",
            0x52540006,
            crate::codegen_abi::native_layout_word("ITERATOR_RESULT_DONE_FIRST", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[(ATOM_26.hash, 0, 0), (ATOM_25.hash, 1, 0)],
            &[u32::MAX, 0, 1, u32::MAX],
        );
        check(
            std::ptr::addr_of!(TIMEOUT),
            "TIMEOUT",
            0x52540007,
            crate::codegen_abi::native_layout_word("TIMEOUT", 1),
            &raw const TIMEOUT_PROTO_FAMILY,
            &[],
            &[u32::MAX, u32::MAX],
        );
        check(
            std::ptr::addr_of!(IMMEDIATE),
            "IMMEDIATE",
            0x52540008,
            crate::codegen_abi::native_layout_word("IMMEDIATE", 1),
            &raw const IMMEDIATE_PROTO_FAMILY,
            &[],
            &[u32::MAX, u32::MAX],
        );
        check(
            std::ptr::addr_of!(REGEX_EXEC_NAMED),
            "REGEX_EXEC_NAMED",
            0x52540009,
            crate::codegen_abi::native_layout_word("REGEX_EXEC_NAMED", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[
                (ATOM_SYMBOL_ITERATOR.hash, 0, 0),
                (ATOM_27.hash, 1, 0),
                (ATOM_28.hash, 2, 0),
                (ATOM_29.hash, 3, 0),
            ],
            &[u32::MAX, u32::MAX, u32::MAX, 1, 2, 0, 3, u32::MAX],
        );
        check(
            std::ptr::addr_of!(REGEX_EXEC_INDICES_NAMED),
            "REGEX_EXEC_INDICES_NAMED",
            0x5254000A,
            crate::codegen_abi::native_layout_word("REGEX_EXEC_INDICES_NAMED", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[
                (ATOM_SYMBOL_ITERATOR.hash, 0, 0),
                (ATOM_27.hash, 1, 0),
                (ATOM_28.hash, 2, 0),
                (ATOM_29.hash, 3, 0),
                (ATOM_30.hash, 4, 0),
            ],
            &[
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
            ],
        );
        check(
            std::ptr::addr_of!(REGEX_INDICES_GROUPS_NAMED),
            "REGEX_INDICES_GROUPS_NAMED",
            0x5254000B,
            crate::codegen_abi::native_layout_word("REGEX_INDICES_GROUPS_NAMED", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[(ATOM_SYMBOL_ITERATOR.hash, 0, 0), (ATOM_29.hash, 1, 0)],
            &[u32::MAX, 0, 1, u32::MAX],
        );
        check(
            std::ptr::addr_of!(ARRAY_NAMED),
            "ARRAY_NAMED",
            0x5254000D,
            crate::codegen_abi::native_layout_word("ARRAY_NAMED", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[(ATOM_SYMBOL_ITERATOR.hash, 0, 0)],
            &[u32::MAX, 0],
        );
        check(
            std::ptr::addr_of!(SQLITE_COLUMN_METADATA),
            "SQLITE_COLUMN_METADATA",
            0x53514c01,
            crate::codegen_abi::native_layout_word("SQLITE_COLUMN_METADATA", 0),
            crate::object::layout_desc::LAYOUT_PROTO_OBJECT,
            &[
                (SQLITE_ATOM_0.hash, 0, 0),
                (SQLITE_ATOM_1.hash, 1, 0),
                (SQLITE_ATOM_2.hash, 2, 0),
                (SQLITE_ATOM_3.hash, 3, 0),
                (SQLITE_ATOM_4.hash, 4, 0),
            ],
            &[
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
            ],
        );
    }
}

pub(super) unsafe fn check(
    desc: *const PerryLayoutDesc,
    name: &str,
    family: u32,
    word: u32,
    proto: *const crate::object::layout_desc::PerryProtoFamily,
    keys: &[(u64, u32, u8)],
    index: &[u32],
) {
    let d = unsafe { &*desc };
    assert_eq!(
        (d.family, d.variant, d.kind, d.integrity, d.rep),
        (family, 0, 0, 0, 0),
        "{name}"
    );
    assert_eq!(
        (d.layout_id, d.slot_count, d.key_count),
        (word, keys.len() as u32, keys.len() as u32),
        "{name}"
    );
    assert_eq!(d.proto, proto, "{name}");
    assert_eq!((d.class_id, d.parent_class_id), (0, 0), "{name}");
    assert!(d.methods.is_null(), "{name}");
    // The two legacy empty layouts carried two vacant buckets. The common
    // emitter uses one for an empty key set; both answer every lookup absent.
    let index = if keys.is_empty() {
        assert_eq!(index, &[u32::MAX, u32::MAX], "{name}");
        &index[..1]
    } else {
        index
    };
    assert_eq!(d.index_mask as usize + 1, index.len(), "{name}");
    assert_eq!(
        unsafe { std::slice::from_raw_parts(d.index, index.len()) },
        index,
        "{name}"
    );
    for (actual, expected) in unsafe { std::slice::from_raw_parts(d.keys, keys.len()) }
        .iter()
        .zip(keys)
    {
        assert_eq!(
            (unsafe { (*actual.atom).hash }, actual.slot, actual.attrs),
            *expected,
            "{name}"
        );
    }
}
