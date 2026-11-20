// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
pub use tempfile::TempDir;

#[allow(unused)]
pub fn tmpdir() -> TempDir {
    tempfile::tempdir().expect("expected to be able to create a temporary directory")
}
