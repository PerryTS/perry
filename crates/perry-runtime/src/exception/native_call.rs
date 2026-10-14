//! A native call's JavaScript callbacks. Each callback runs under its own
//! runtime trap (the landing-pad shim; setjmp on WASI), whose pad restores
//! every managed stack to the trap's frame. Between callbacks nothing is
//! armed, and nothing needs to be: a throw there is an ordinary runtime
//! throw, restored by whichever pad catches it.

use super::*;

/// Run one callback of a native call under a trap. All pointer values are
/// already rooted by the caller.
pub(crate) unsafe fn catch_native_callback(
    callee: f64,
    this: f64,
    args: &[f64],
) -> Result<f64, f64> {
    // POD context avoids generic FnOnce/Option transport on every callback.
    struct Invocation {
        callee: f64,
        this: f64,
        args: *const f64,
        len: usize,
        result: f64,
    }
    unsafe extern "C-unwind" fn invoke(raw: *mut core::ffi::c_void) {
        let ctx = &mut *(raw as *mut Invocation);
        let this = crate::closure::JsThis::from_f64(ctx.this);
        // A function whose body the compiler emitted (every arrow or
        // function a program registers) enters that body directly; any
        // other callee takes the generic value call.
        ctx.result = match compiled_body(ctx.callee) {
            Some((closure, info)) => {
                crate::closure::call_compiled_body_this(closure, info, this, ctx.args, ctx.len)
            }
            None => crate::closure::native_call_value_this(ctx.callee, this, ctx.args, ctx.len),
        };
    }
    let mut ctx = Invocation {
        callee,
        this,
        args: args.as_ptr(),
        len: args.len(),
        result: 0.0,
    };
    if trap_raw::<false>(invoke, (&raw mut ctx).cast()) {
        Ok(ctx.result)
    } else {
        Err(take_exception())
    }
}

/// The closure `callee` names and its body when the compiler emitted that
/// body (`FN_COMPILED_BODY`): none of the exotic callees the generic value
/// call tests first (class refs, proxies, bound native exports, no-op-backed
/// built-ins, class objects) carries it.
#[inline]
unsafe fn compiled_body(
    callee: f64,
) -> Option<(
    *const crate::closure::ClosureHeader,
    &'static crate::closure::JsFunctionInfo,
)> {
    let bits = callee.to_bits();
    if bits & crate::value::TAG_MASK != crate::value::POINTER_TAG {
        return None;
    }
    let ptr = (bits & crate::value::POINTER_MASK) as usize;
    let header = crate::value::addr_class::try_read_gc_header(ptr)?;
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE
        || header.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
    {
        return None;
    }
    let closure = ptr as *const crate::closure::ClosureHeader;
    let info = (*closure).info.as_ref()?;
    (info.flags & crate::codegen_abi::FN_COMPILED_BODY != 0).then_some((closure, info))
}
