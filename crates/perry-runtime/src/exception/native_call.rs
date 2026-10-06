//! Reuse a lazily captured savepoint during one native call. Jump targets
//! remain trampoline-local; between callbacks the handler is disarmed.

use super::*;

pub(crate) struct NativeCatch {
    depth: Option<usize>,
    #[cfg(test)]
    captures: u32,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl NativeCatch {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            depth: None,
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
        if let Some(depth) = self.depth.take() {
            with_exception_state(|s| unsafe {
                assert_eq!((*s).try_depth, depth + 1, "unbalanced native callback trap");
                assert_eq!((*s).handler_kinds[depth], HandlerKind::NativeInactive);
                #[cfg(test)]
                if crate::native_payload::callback_sabotage("catch_pop") {
                    return;
                }
                (*s).try_depth = depth;
            });
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
    let env = with_exception_state(|s| {
        let depth = if let Some(depth) = (*catch).depth {
            assert_eq!((*s).try_depth, depth + 1, "unbalanced callback entry");
            (&mut (*s).savepoints)
                .get_unchecked_mut(depth)
                .assume_init_mut()
                .refresh_native_roots();
            depth
        } else {
            // Capture here, after the first callback's argument roots exist.
            // No callback means no capture or handler stack mutation.
            let env = try_push_with_kind(HandlerKind::NativeInactive);
            let depth = (*s).try_depth - 1;
            (*catch).depth = Some(depth);
            #[cfg(test)]
            {
                (*catch).captures += 1;
            }
            (*s).handler_kinds[depth] = HandlerKind::Setjmp;
            return env;
        };
        (*s).handler_kinds[depth] = HandlerKind::Setjmp;
        (&mut (*s).jump_buffers)
            .get_unchecked_mut(depth)
            .as_mut_ptr()
    });
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
        ctx.result = crate::closure::native_call_value_this(
            ctx.callee,
            crate::closure::JsThis::from_f64(ctx.this),
            ctx.args,
            ctx.len,
        );
    }
    let mut ctx = Invocation {
        callee,
        this,
        args: args.as_ptr(),
        len: args.len(),
        result: 0.0,
    };
    let rc = perry_sjlj_try(env.cast(), invoke, (&raw mut ctx).cast());
    with_exception_state(|s| {
        let depth = (*catch).depth.unwrap_unchecked();
        assert_eq!((*s).try_depth, depth + 1, "unbalanced callback return");
        (*s).handler_kinds[depth] = HandlerKind::NativeInactive;
    });
    if rc == 0 {
        Ok(ctx.result)
    } else {
        let err = js_get_exception();
        js_clear_exception();
        Err(err)
    }
}
