fn main() {
    println!("cargo:rerun-if-env-changed=LLVM_SYS_221_PREFIX");
}
