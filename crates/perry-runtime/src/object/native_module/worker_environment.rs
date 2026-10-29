use super::super::native_module_dispatch::NmCtx;
use crate::feature_hooks::Hook;

static GET: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
static SET: Hook<extern "C" fn(f64, f64) -> f64> = Hook::empty();

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
