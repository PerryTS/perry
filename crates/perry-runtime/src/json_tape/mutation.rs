/// Materialize a lazy receiver before applying ordinary computed assignment.
///
/// Materialization turns the receiver into the ordinary array in place, so
/// the assignment then runs on the very same object.
///
/// # Safety
/// `header` must be a live GC_TYPE_LAZY_ARRAY header. The dynamic dispatcher
/// proves the receiver type before entering this cold path.
pub(crate) unsafe fn set_lazy_index(
    header: *mut super::LazyArrayHeader,
    index: f64,
    value: f64,
    strict: i32,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let index = scope.root_nanbox_f64(index);
    let value = scope.root_nanbox_f64(value);
    let array = super::force_materialize_lazy(header);
    // No allocation lies between this fresh pointer and the ordinary setter,
    // which retains key/strict behavior.
    crate::value::js_dyn_index_set_strict(
        f64::from_bits(crate::JSValue::object_ptr(array.cast()).bits()),
        index.get_nanbox_f64(),
        value.get_nanbox_f64(),
        strict,
    );
    value.get_nanbox_f64()
}
