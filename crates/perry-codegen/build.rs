fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let prefix = std::env::var_os("LLVM_SYS_221_PREFIX").unwrap_or_else(|| {
        panic!("LLVM_SYS_221_PREFIX must point to the LLVM 22 development archive on Windows")
    });
    let lib_dir = std::path::PathBuf::from(prefix).join("lib");
    if !lib_dir.join("LLVM-C.lib").is_file() {
        panic!("{} does not contain LLVM-C.lib", lib_dir.display());
    }

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=dylib=LLVM-C");
}
