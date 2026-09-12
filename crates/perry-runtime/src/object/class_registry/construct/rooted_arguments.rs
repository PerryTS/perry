use super::*;

/// Construct with argument slots the collector can rewrite even while native
/// constructor setup allocates before entering the JavaScript body. The trap
/// sits below both the buffer owner and shadow frame, so JS throws release
/// storage and restore both new.target cells as well as implicit this.
pub(crate) fn construct_rooted_arguments(function: f64, args: &[f64], new_target: f64) -> f64 {
    use std::cell::UnsafeCell;
    let scope = crate::gc::RuntimeHandleScope::new();
    let function = scope.root_nanbox_f64(function);
    let new_target = scope.root_nanbox_f64(new_target);
    let previous_this = scope.root_nanbox_f64(crate::object::js_implicit_this_get());
    let previous_target = scope.root_nanbox_f64(crate::object::js_new_target_get());
    let previous_current = scope.root_nanbox_f64(js_new_target_value());
    let args: Vec<UnsafeCell<f64>> = args.iter().copied().map(UnsafeCell::new).collect();
    struct Frame(u64);
    impl Drop for Frame {
        fn drop(&mut self) {
            crate::gc::js_shadow_frame_pop(self.0);
        }
    }
    let frame = Frame(crate::gc::js_shadow_frame_push(
        args.len().try_into().expect("argument count fits u32"),
    ));
    for (index, value) in args.iter().enumerate() {
        crate::gc::js_shadow_slot_bind(index as u32, value.get().cast());
    }
    let result = crate::exception::catch_js_throw(|| unsafe {
        js_new_function_construct_with_new_target(
            function.get_nanbox_f64(),
            args.as_ptr().cast(),
            args.len(),
            new_target.get_nanbox_f64(),
        )
    });
    CURRENT_NEW_TARGET.with(|cell| cell.set(previous_current.get_nanbox_f64().to_bits()));
    crate::object::js_new_target_set(previous_target.get_nanbox_f64());
    crate::object::js_implicit_this_set(previous_this.get_nanbox_f64());
    drop(frame);
    drop(args);
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

/// Only the regular-expression species constructors construct with exactly two
/// rooted arguments, so a build without the engine has no caller.
#[cfg(feature = "regex-engine")]
pub(crate) fn construct_two_rooted(function: f64, first: f64, second: f64) -> f64 {
    construct_rooted_arguments(function, &[first, second], function)
}
