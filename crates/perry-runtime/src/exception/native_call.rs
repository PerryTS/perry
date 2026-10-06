//! Reuse a lazily captured savepoint during one native call. Jump targets
//! remain trampoline-local; between callbacks the handler is disarmed.

use super::*;

pub(crate) struct NativeCatch {
    /// This thread's exception state, once a callback captured the handler
    /// (null until then). Stable for the thread's life.
    state: *mut ExceptionState,
    /// The handler slot the first callback pushed.
    depth: usize,
    #[cfg(test)]
    captures: u32,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl NativeCatch {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            state: std::ptr::null_mut(),
            depth: 0,
            #[cfg(test)]
            captures: 0,
            _thread: std::marker::PhantomData,
        }
    }

    #[cfg(test)]
    pub(crate) fn captures(&self) -> u32 {
        self.captures
    }

    pub(crate) fn finish(&mut self) {
        let s = std::mem::replace(&mut self.state, std::ptr::null_mut());
        if s.is_null() {
            return;
        }
        let depth = self.depth;
        unsafe {
            assert_eq!((*s).try_depth, depth + 1, "unbalanced native callback trap");
            assert_eq!((*s).handler_kinds[depth], HandlerKind::NativeInactive);
            #[cfg(test)]
            if crate::native_payload::callback_sabotage("catch_pop") {
                return;
            }
            (*s).try_depth = depth;
        }
    }
}

/// `catch` is null for a legacy entry, or points at the current native call's
/// stack token. All pointer values are already rooted by the caller.
pub(crate) unsafe fn catch_native_callback(
    catch: *mut NativeCatch,
    callee: f64,
    this: f64,
    args: &[f64],
) -> Result<f64, f64> {
    // Keep the ordinary full savepoint for entries not using guard.call.
    if catch.is_null() {
        return catch_js_throw(|| {
            crate::closure::native_call_value_this(
                callee,
                crate::closure::JsThis::from_f64(this),
                args.as_ptr(),
                args.len(),
            )
        });
    }
    let s = (*catch).state;
    let (s, depth) = if s.is_null() {
        // Capture here, after the first callback's argument roots exist.
        // No callback means no capture or handler stack mutation.
        try_push_with_kind(HandlerKind::NativeInactive);
        let s = with_exception_state(|s| s);
        let depth = (*s).try_depth - 1;
        (*catch).state = s;
        (*catch).depth = depth;
        #[cfg(test)]
        {
            (*catch).captures += 1;
        }
        (s, depth)
    } else {
        let depth = (*catch).depth;
        assert_eq!((*s).try_depth, depth + 1, "unbalanced callback entry");
        // Root scopes may differ in each trampoline's argument conversion.
        (&mut (*s).savepoints)
            .get_unchecked_mut(depth)
            .assume_init_mut()
            .refresh_native_roots();
        (s, depth)
    };
    *(&mut (*s).handler_kinds).get_unchecked_mut(depth) = HandlerKind::Setjmp;
    let env = (&mut (*s).jump_buffers)
        .get_unchecked_mut(depth)
        .as_mut_ptr();
    // POD context avoids generic FnOnce/Option transport on every callback.
    struct Invocation {
        callee: f64,
        this: f64,
        args: *const f64,
        len: usize,
        result: f64,
    }
    unsafe extern "C" fn invoke(raw: *mut core::ffi::c_void) {
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
    let rc = perry_sjlj_try(env.cast(), invoke, (&raw mut ctx).cast());
    assert_eq!((*s).try_depth, depth + 1, "unbalanced callback return");
    *(&mut (*s).handler_kinds).get_unchecked_mut(depth) = HandlerKind::NativeInactive;
    if rc == 0 {
        Ok(ctx.result)
    } else {
        let err = js_get_exception();
        js_clear_exception();
        Err(err)
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
