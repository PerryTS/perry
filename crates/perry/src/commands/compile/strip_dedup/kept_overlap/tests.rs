use super::*;
use object::{Object, ObjectSymbol};
use std::collections::HashSet;

// x86_64 Mach-O object compiled from four extern functions: `shared_one`,
// `shared_two`, `ring_core_0_17_14__OPENSSL_cpuid_setup` and `needed_by_sibling`.
const KEPT_MEMBER_HEX: &str = concat!(
    "cffaedfe0700000103000000010000000400000018010000002000000000000019000000980000000000000000000000",
    "000000000000000000000000000000003b0000000000000038010000000000003b000000000000000700000007000000",
    "01000000000000005f5f74657874000000000000000000005f5f54455854000000000000000000000000000000000000",
    "3b0000000000000038010000040000000000000000000000000400800000000000000000000000003200000018000000",
    "0100000000001a0000001b000000000002000000180000007801000004000000b8010000580000000b00000050000000",
    "000000000000000000000000040000000400000000000000000000000000000000000000000000000000000000000000",
    "000000000000000000000000000000000000000000000000554889e5b8010000005dc30f1f440000554889e5b8020000",
    "005dc30f1f440000554889e5b8030000005dc30f1f440000554889e5b8040000005dc30000000000350000000f010000",
    "3000000000000000010000000f0100002000000000000000480000000f0100000000000000000000290000000f010000",
    "1000000000000000005f72696e675f636f72655f305f31375f31345f5f4f50454e53534c5f63707569645f7365747570",
    "005f7368617265645f74776f005f6e65656465645f62795f7369626c696e67005f7368617265645f6f6e650000000000",
);

// x86_64 Mach-O object compiled from two extern functions over one internal
// static: `shared_register` writes it and `needed_by_sibling` reads it.
const STATEFUL_MEMBER_HEX: &str = concat!(
    "cffaedfe0700000103000000010000000400000068010000002000000000000019000000e80000000000000000000000",
    "00000000000000000000000000000000210000000000000088010000000000001f000000000000000700000007000000",
    "02000000000000005f5f74657874000000000000000000005f5f54455854000000000000000000000000000000000000",
    "1f000000000000008801000004000000a801000002000000000400800000000000000000000000005f5f627373000000",
    "00000000000000005f5f4441544100000000000000000000200000000000000001000000000000000000000002000000",
    "00000000000000000100000000000000000000000000000032000000180000000100000000000b0000021b0000000000",
    "0200000018000000b801000003000000e8010000380000000b0000005000000000000000010000000100000002000000",
    "030000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000554889e5c605ffffffff015dc30f1f00554889e50fb60500000000ffc05dc300170000000000001d",
    "060000000000006d250000000e0200002000000000000000120000000f0100001000000000000000010000000f010000",
    "0000000000000000005f7368617265645f7265676973746572005f6e65656465645f62795f7369626c696e67005f7265",
    "67697374657265640000000000000000",
);

fn decode_hex(hex: &str) -> Vec<u8> {
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("fixture is valid hex"))
        .collect()
}

fn symbol_set(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

fn global_defined_symbols(object_bytes: &[u8]) -> Vec<String> {
    let file = object::File::parse(object_bytes).expect("localized member stays a valid object");
    let mut globals: Vec<String> = file
        .symbols()
        .filter(|symbol| symbol.is_definition() && symbol.is_global())
        .map(|symbol| symbol.name().expect("symbol name").to_string())
        .collect();
    globals.sort();
    globals
}

#[test]
fn kept_member_stops_exporting_symbols_its_stdlib_replacement_provides() {
    let objcopy = find_nightly_llvm_tool("llvm-objcopy")
        .or_else(|| find_llvm_tool("llvm-objcopy"))
        .expect("llvm-objcopy is required to localize Mach-O exports");
    let extract_dir = tempfile::tempdir().unwrap();
    let member = "kept_codegen_unit.o";
    let member_path = extract_dir.path().join(member);
    let original = decode_hex(KEPT_MEMBER_HEX);
    std::fs::write(&member_path, &original).unwrap();

    let exported = symbol_set(&[
        "_shared_one",
        "_shared_two",
        "_ring_core_0_17_14__OPENSSL_cpuid_setup",
        "_needed_by_sibling",
    ]);
    let replacement = symbol_set(&[
        "_shared_one",
        "_shared_two",
        "_ring_core_0_17_14__OPENSSL_cpuid_setup",
        "_only_in_stdlib",
    ]);
    assert_eq!(global_defined_symbols(&original).len(), exported.len());

    let localized = localize_provided_by_replacement(
        &objcopy,
        extract_dir.path(),
        member,
        &exported,
        &replacement,
    )
    .unwrap();

    assert_eq!(localized, 2);
    assert_eq!(
        global_defined_symbols(&std::fs::read(&member_path).unwrap()),
        [
            "_needed_by_sibling",
            "_ring_core_0_17_14__OPENSSL_cpuid_setup"
        ]
    );
}

#[test]
fn kept_member_with_mutable_state_keeps_every_export() {
    let objcopy = find_nightly_llvm_tool("llvm-objcopy")
        .or_else(|| find_llvm_tool("llvm-objcopy"))
        .expect("llvm-objcopy is required to localize Mach-O exports");
    let extract_dir = tempfile::tempdir().unwrap();
    let member = "kept_stateful_unit.o";
    let member_path = extract_dir.path().join(member);
    let original = decode_hex(STATEFUL_MEMBER_HEX);
    std::fs::write(&member_path, &original).unwrap();

    let exported = symbol_set(&["_shared_register", "_needed_by_sibling"]);
    let replacement = symbol_set(&["_shared_register", "_only_in_stdlib"]);

    let localized = localize_provided_by_replacement(
        &objcopy,
        extract_dir.path(),
        member,
        &exported,
        &replacement,
    )
    .unwrap();

    assert_eq!(localized, 0);
    assert_eq!(
        global_defined_symbols(&std::fs::read(&member_path).unwrap()),
        ["_needed_by_sibling", "_shared_register"]
    );
}
