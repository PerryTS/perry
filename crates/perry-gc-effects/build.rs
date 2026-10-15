fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let prefix = std::env::var_os("LLVM_SYS_221_PREFIX")
            .expect("LLVM_SYS_221_PREFIX must point to LLVM 22 on Windows");
        let lib = std::path::PathBuf::from(prefix).join("lib");
        assert!(lib.join("LLVM-C.lib").is_file(), "missing LLVM-C.lib");
        println!("cargo:rustc-link-search=native={}", lib.display());
        println!("cargo:rustc-link-lib=dylib=LLVM-C");
    }
}
