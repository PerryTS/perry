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
}
