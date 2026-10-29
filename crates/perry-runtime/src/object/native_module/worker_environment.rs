use super::super::native_module_dispatch::NmCtx;
use super::*;

static GET: AtomicPtr<()> = AtomicPtr::new(null_mut());
static SET: AtomicPtr<()> = AtomicPtr::new(null_mut());

#[no_mangle]
pub extern "C" fn js_register_worker_threads_environment_data(
    get: extern "C" fn(f64) -> f64,
    set: extern "C" fn(f64, f64) -> f64,
) {
    GET.store(get as *mut (), Ordering::Release);
    SET.store(set as *mut (), Ordering::Release);
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
            let ptr = GET.load(Ordering::Acquire);
            if !ptr.is_null() {
                let get: extern "C" fn(f64) -> f64 = std::mem::transmute(ptr);
                return get(arg(0));
            }
        }
        "setEnvironmentData" => {
            let ptr = SET.load(Ordering::Acquire);
            if !ptr.is_null() {
                let set: extern "C" fn(f64, f64) -> f64 = std::mem::transmute(ptr);
                return set(arg(0), arg(1));
            }
        }
        _ => {}
    }
    crate::collection_iter::throw_type_error(&format!("{module}.{method} is not supported"))
}
