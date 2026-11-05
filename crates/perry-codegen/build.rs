#[allow(dead_code)] // The schema also declares the compiler-only borrowed table row.
#[path = "../perry-runtime-manifest/src/realm.rs"]
mod realm;
#[path = "../perry-runtime-manifest/src/realm_codegen.rs"]
mod realm_codegen;

fn main() {
    let manifest_dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    realm_codegen::generate(&manifest_dir.join("../perry-runtime/src"), &out)
        .expect("generate compiler realm table from runtime declarations");

    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
    println!("cargo:rerun-if-changed=src/wasm32/target_options.cpp");
    println!("cargo:rerun-if-changed=src/inprocess/unique_sections.cpp");
    let wasi = std::env::var_os("CARGO_FEATURE_TARGET_WASI").is_some();
    let inprocess = std::env::var_os("CARGO_FEATURE_LLVM_INPROCESS").is_some();
    if wasi || inprocess {
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
        let mut build = cc::Build::new();
        build
            .cpp(true)
            .include(include)
            .flag_if_supported("-std=c++17")
            .flag_if_supported("/std:c++17");
        if wasi {
            build.file("src/wasm32/target_options.cpp");
        }
        if inprocess {
            build.file("src/inprocess/unique_sections.cpp");
        }
        build.compile("perry_llvm_target_options");
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
