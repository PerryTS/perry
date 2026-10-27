// Adapted from Wasmtime 48.0.5; Apache-2.0 WITH LLVM-exception. See PROVENANCE.md.
#[derive(Debug, Clone)]
pub(crate) struct DirOptionsExt {}

impl DirOptionsExt {
    pub(crate) const fn new() -> Self {
        Self {}
    }
}
