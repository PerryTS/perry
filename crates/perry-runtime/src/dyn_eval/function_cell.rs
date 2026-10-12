//! Capture zero owns a compact, collecting cell containing one native Rc.
//! The pure Rust AST has no JS edges. Its cache and node memos never root GC.
use super::*;
use crate::value::JSValue;
#[repr(C)]
struct FunctionCell {
    function: *const InterpFn,
}
pub(super) fn new(function: Rc<InterpFn>) -> f64 {
    let _publication = crate::gc::GcSuppressScope::new();
    unsafe {
        let cell = crate::gc::gc_malloc(
            std::mem::size_of::<FunctionCell>(),
            crate::gc::GC_TYPE_INTERP_FUNCTION,
        )
        .cast::<FunctionCell>();
        std::ptr::write(
            cell,
            FunctionCell {
                function: Rc::into_raw(function),
            },
        );
        crate::value::js_nanbox_pointer(cell as i64)
    }
}
pub(super) fn get(value: f64) -> Option<Rc<InterpFn>> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    unsafe {
        let cell = value.as_pointer::<FunctionCell>();
        crate::value::addr_class::try_read_tracked_gc_header_of_type(
            cell as usize,
            crate::gc::GC_TYPE_INTERP_FUNCTION,
        )?;
        let function = (*cell).function;
        if function.is_null() {
            return None;
        }
        Rc::increment_strong_count(function);
        Some(Rc::from_raw(function))
    }
}
/// Basic native cleanup only; no GC allocation, JS callbacks or TLS access.
pub(crate) unsafe fn finalize(user: *mut u8) {
    let cell = user.cast::<FunctionCell>();
    let function = std::mem::replace(&mut (*cell).function, std::ptr::null());
    if !function.is_null() {
        drop(Rc::from_raw(function));
    }
}
