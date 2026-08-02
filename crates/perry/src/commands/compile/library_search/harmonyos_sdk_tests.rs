use super::is_usable_harmonyos_native_sdk;

#[test]
fn native_sdk_requires_both_clang_and_sysroot() {
    let sdk = tempfile::tempdir().expect("tempdir");
    let bin = sdk.path().join("llvm/bin");
    std::fs::create_dir_all(&bin).expect("create llvm bin");
    assert!(!is_usable_harmonyos_native_sdk(sdk.path()));

    std::fs::write(bin.join("clang"), b"").expect("create clang");
    assert!(!is_usable_harmonyos_native_sdk(sdk.path()));

    std::fs::create_dir(sdk.path().join("sysroot")).expect("create sysroot");
    assert!(is_usable_harmonyos_native_sdk(sdk.path()));
}
