//! Give isolated stdlib tests the same startup as an emitted program.
pub(crate) fn initialize() {
    perry_runtime::gc::gc_init();
    unsafe extern "C" {
        fn js_shape_run_static_seed();
    }
    unsafe { js_shape_run_static_seed() };
}
