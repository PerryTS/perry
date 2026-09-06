/// Materialize a lazy receiver before applying ordinary computed assignment.
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
    let receiver = scope.root_raw_mut_ptr(header);
    let index = scope.root_nanbox_f64(index);
    let value = scope.root_nanbox_f64(value);
    let array = super::force_materialize_lazy(receiver.get_raw_mut_ptr());
    // The owner roots its materialized array. No allocation lies between this
    // fresh pointer and the ordinary setter, which retains key/strict behavior.
    crate::value::js_dyn_index_set_strict(
        f64::from_bits(crate::JSValue::object_ptr(array.cast()).bits()),
        index.get_nanbox_f64(),
        value.get_nanbox_f64(),
        strict,
    );
    // Length reads can still arrive through the original lazy value. Reflect
    // indexed extension or a "length" assignment in its inline length slot.
    let header = receiver.get_raw_mut_ptr::<super::LazyArrayHeader>();
    (*header).cached_length = crate::array::js_array_length((*header).materialized);
    value.get_nanbox_f64()
}
