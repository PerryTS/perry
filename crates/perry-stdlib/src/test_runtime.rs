//! Standalone stdlib fixtures use the runtime's shared startup path.
pub(crate) fn initialize() {
    perry_runtime::gc::initialize_runtime_fixture();
}
