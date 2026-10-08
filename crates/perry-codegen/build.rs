fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    println!("cargo:rerun-if-changed=src/wasm32/target_options.cpp");
    if std::env::var_os("CARGO_FEATURE_TARGET_WASI").is_some() {
        let include = if let Some(prefix) = std::env::var_os("LLVM_SYS_221_PREFIX") {
            std::path::PathBuf::from(prefix).join("include")
        } else {
            let output = std::process::Command::new("llvm-config")
                .arg("--includedir")
                .output()
                .expect("llvm-config is required to locate the LLVM headers");
            assert!(output.status.success(), "llvm-config --includedir failed");
            std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
        };
        cc::Build::new()
            .cpp(true)
            .include(include)
            .file("src/wasm32/target_options.cpp")
            .flag_if_supported("-std=c++17")
            .flag_if_supported("/std:c++17")
            .compile("perry_wasm_target_options");
    }

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
