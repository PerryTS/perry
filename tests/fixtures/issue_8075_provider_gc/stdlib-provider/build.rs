fn main() {
    // The final linker creates the runtime dylib beside the stdlib output.
    // A new gate scratch path or linker implementation must force that link,
    // even when Cargo can reuse every dependency and the stdlib's Rust code.
    for key in [
        "PERRY_ISSUE_8075_RUNTIME_LIBRARY",
        "PERRY_ISSUE_8075_REAL_CC",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed=../stdlib-linker.sh");
    println!("cargo:rerun-if-changed=../stdlib-linker.py");
}
