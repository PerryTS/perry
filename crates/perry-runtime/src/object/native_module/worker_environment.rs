use super::super::native_module_dispatch::NmCtx;
use crate::feature_hooks::Hook;

static GET: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
static SET: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();
type CallableDispatch = unsafe extern "C" fn(*const u8, usize, *const f64, usize, *mut f64) -> bool;
static CALL: Hook<CallableDispatch> = Hook::empty();

#[no_mangle]
pub extern "C" fn js_register_worker_threads_callable_dispatch(dispatch: CallableDispatch) {
    CALL.set(dispatch);
}

#[no_mangle]
pub extern "C" fn js_register_worker_threads_environment_data(
    get: extern "C" fn(f64) -> f64,
    set: extern "C" fn(f64, f64) -> f64,
) {
    GET.set(get);
    SET.set(set);
}

pub(crate) unsafe fn dispatch(ctx: &NmCtx, module: &str, method: &str) -> f64 {
    let arg = |index| {
        if index < ctx.args_len && !ctx.args_ptr.is_null() {
            *ctx.args_ptr.add(index)
        } else {
            f64::from_bits(crate::value::TAG_UNDEFINED)
        }
    };
    match method {
        "getEnvironmentData" => {
            if let Some(get) = GET.peek() {
                return get(arg(0));
            }
        }
        "setEnvironmentData" => {
            if let Some(set) = SET.peek() {
                return set(arg(0), arg(1));
            }
        }
        // The clone markers live in the runtime: the same answer as the direct
        // `worker_threads.markAs*(x)` call the codegen lowers to perry-stdlib
        // (a bundled `const { markAsUncloneable } = require("node:worker_threads")`
        // reaches this namespace path; undici's CacheStorage calls it at init).
        "markAsUncloneable" => {
            crate::clone_markers::mark_uncloneable(arg(0));
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        "markAsUntransferable" => {
            crate::clone_markers::mark_untransferable(arg(0));
            return f64::from_bits(crate::value::TAG_UNDEFINED);
        }
        "isMarkedAsUntransferable" => {
            return f64::from_bits(if crate::clone_markers::is_untransferable(arg(0)) {
                crate::value::TAG_TRUE
            } else {
                crate::value::TAG_FALSE
            });
        }
        "receiveMessageOnPort" | "postMessageToThread" => {
            if let Some(call) = CALL.peek() {
                let mut value = f64::from_bits(crate::value::TAG_UNDEFINED);
                if call(
                    method.as_ptr(),
                    method.len(),
                    ctx.args_ptr,
                    ctx.args_len,
                    &mut value,
                ) {
                    return value;
                }
            }
        }
        _ => {}
    }
    crate::collection_iter::throw_type_error(&format!("{module}.{method} is not supported"))
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn get(key: f64) -> f64 {
        key
    }
    extern "C" fn set(_key: f64, value: f64) -> f64 {
        value
    }

    /// `require("node:worker_threads").markAsUncloneable(x)` through the
    /// namespace object (a bundle's destructured import) sets the same runtime
    /// clone markers as the lowered direct call, instead of throwing.
    #[test]
    fn namespace_clone_marker_calls_set_runtime_markers() {
        crate::clone_markers::reset_for_tests();
        let object = crate::object::js_object_alloc(0, 0);
        let value = crate::value::js_nanbox_pointer(object as i64);
        let args = [value];
        let ctx = NmCtx {
            obj: std::ptr::null(),
            args_ptr: args.as_ptr(),
            args_len: args.len(),
            assert_skip_prototype: false,
        };
        unsafe {
            assert_eq!(
                dispatch(&ctx, "worker_threads", "isMarkedAsUntransferable").to_bits(),
                crate::value::TAG_FALSE
            );
            assert_eq!(
                dispatch(&ctx, "worker_threads", "markAsUncloneable").to_bits(),
                crate::value::TAG_UNDEFINED
            );
            assert!(crate::clone_markers::is_uncloneable(value));
            dispatch(&ctx, "worker_threads", "markAsUntransferable");
            assert_eq!(
                dispatch(&ctx, "worker_threads", "isMarkedAsUntransferable").to_bits(),
                crate::value::TAG_TRUE
            );
        }
    }

    #[test]
    fn registered_hooks_preserve_arguments_and_missing_values() {
        js_register_worker_threads_environment_data(get, set);
        let args = [42.0, 7.0];
        let mut ctx = NmCtx {
            obj: std::ptr::null(),
            args_ptr: args.as_ptr(),
            args_len: args.len(),
            assert_skip_prototype: false,
        };
        unsafe {
            assert_eq!(dispatch(&ctx, "worker_threads", "getEnvironmentData"), 42.0);
            assert_eq!(dispatch(&ctx, "worker_threads", "setEnvironmentData"), 7.0);
            ctx.args_ptr = std::ptr::null();
            ctx.args_len = 0;
            for method in ["getEnvironmentData", "setEnvironmentData"] {
                assert_eq!(
                    dispatch(&ctx, "worker_threads", method).to_bits(),
                    crate::value::TAG_UNDEFINED
                );
            }
        }
    }
}
