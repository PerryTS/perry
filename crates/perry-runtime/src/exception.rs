//! Exception handling runtime for Perry.
//!
//! One transport on native targets (#7302, ehfree slices 2b and 1): every
//! handler is a landing pad that the personality finds, and nothing is
//! recorded when a handler is entered.
//!
//! * **Generated `try`/`catch`**: the frame's `landingpad` receives control
//!   (see `crate::eh`). Entering and leaving the protected region costs
//!   nothing: there is no handler stack and no savepoint.
//! * **Runtime traps** ([`trap`], behind [`catch_js_throw`],
//!   [`js_call_catching`], the drains and [`js_trap_call`]): runtime helpers
//!   that drive user JS from a Rust-owned context run it through the
//!   checked-in shim `ffi/perry_trap.ll`, an `invoke` of the body under a
//!   catch-all pad with Perry's personality — the same shape as a generated
//!   `try`.
//!
//! Every pad (generated or shim) first calls [`js_eh_pad_enter`] with its own
//! stack pointer, which pops the managed-stack entries of the frames the throw
//! abandoned (`marks`: each entry carries the address of the frame that owns
//! it). `js_throw` stores the value and raises (the single-phase walker, then
//! `_Unwind_RaiseException`, or `RaiseException` on Windows). When no handler
//! exists the raise returns `_URC_END_OF_STACK` without unwinding (Windows:
//! the unhandled-exception filter runs with the stack intact), and the
//! uncaught path runs on the intact stack.
//!
//! WASI keeps setjmp transport (design §1.3): a generated `try` and a runtime
//! trap push a jump buffer ([`js_try_push`]) that records the frame's mark;
//! `js_throw` restores with the same mark rule and `longjmp`s.

#[cfg(target_os = "wasi")]
mod wasi_handlers {
    // Platform-specific jmp_buf size (in i32 units); conservative.
    pub(super) const JMP_BUF_SIZE: usize = 64; // 64 * i32 = 256 bytes

    #[repr(C, align(16))]
    #[derive(Copy, Clone)]
    pub(super) struct JmpBuf {
        pub(super) data: [i32; JMP_BUF_SIZE],
    }

    impl JmpBuf {
        pub(super) fn as_mut_ptr(&mut self) -> *mut i32 {
            self.data.as_mut_ptr()
        }
    }

    // SAFETY: plain `i32`s, and a slot is armed by `setjmp` before it is ever
    // jumped through.
    unsafe impl crate::zeroed_cache::ZeroEmpty for JmpBuf {}

    // Jump buffers live in fixed-size heap slabs per thread, so published
    // jmp_buf addresses stay stable as the depth grows.
    pub(super) const JUMP_BUFFER_SLAB_SIZE: usize = 256;

    /// What a WASI handler restores besides the mark rule: the stacks that
    /// compiled code pushes in its own frame. A landing pad closes those with
    /// actions; a setjmp handler has no pad, so it records their depths.
    #[derive(Clone, Copy)]
    pub(super) struct WasiHandler {
        pub(super) mark: usize,
        pub(super) handles: usize,
        pub(super) shadow: crate::gc::FrameRootSavepoint,
        pub(super) temp_roots: usize,
        pub(super) derived_super: usize,
    }
}
#[cfg(target_os = "wasi")]
use wasi_handlers::*;

pub(crate) mod marks;
pub(crate) use marks::CatchStack;
pub use marks::{caller_frame_mark, frame_mark};
mod native_call;
pub(crate) use native_call::catch_native_callback;

#[cfg(target_os = "wasi")]
extern "C" {
    #[link_name = "perry_wasi_longjmp"]
    fn longjmp(env: *mut i32, val: i32) -> !;
}

struct ExceptionState {
    #[cfg(target_os = "wasi")]
    jump_buffer_slabs: Vec<Box<[JmpBuf]>>,
    #[cfg(target_os = "wasi")]
    handlers: Vec<WasiHandler>,
    current_exception: f64,
    has_exception: bool,
    in_finally: bool,
    /// Set by [`hand_to_worker_base`] while its raise is in flight; the
    /// worker base trap clears it when it lands.
    #[cfg(not(target_os = "wasi"))]
    to_worker_base: bool,
    /// Debug builds: [`js_eh_pad_enter`] is running. A throw from inside it
    /// would land on half-restored stacks.
    #[cfg(debug_assertions)]
    in_pad_enter: bool,
}

impl ExceptionState {
    fn new() -> Self {
        Self {
            #[cfg(target_os = "wasi")]
            jump_buffer_slabs: vec![crate::zeroed_cache::new_zeroed_cache(JUMP_BUFFER_SLAB_SIZE)],
            #[cfg(target_os = "wasi")]
            handlers: Vec::new(),
            current_exception: 0.0,
            has_exception: false,
            in_finally: false,
            #[cfg(not(target_os = "wasi"))]
            to_worker_base: false,
            #[cfg(debug_assertions)]
            in_pad_enter: false,
        }
    }

    /// The jump buffer of handler `depth`, growing the slabs as needed.
    /// Appending a slab can move the Vec of Box handles, never the separate
    /// heap allocations whose addresses have escaped to setjmp frames.
    #[cfg(target_os = "wasi")]
    fn jump_buffer_ptr(&mut self, depth: usize) -> *mut i32 {
        let slab_index = depth / JUMP_BUFFER_SLAB_SIZE;
        while self.jump_buffer_slabs.len() <= slab_index {
            self.jump_buffer_slabs
                .push(crate::zeroed_cache::new_zeroed_cache(JUMP_BUFFER_SLAB_SIZE));
        }
        self.jump_buffer_slabs[slab_index][depth % JUMP_BUFFER_SLAB_SIZE].as_mut_ptr()
    }
}

crate::perry_thread_local! {
    static EXCEPTION_STATE: std::cell::UnsafeCell<ExceptionState> =
        std::cell::UnsafeCell::new(ExceptionState::new());
}

#[inline]
fn with_exception_state<R>(f: impl FnOnce(*mut ExceptionState) -> R) -> R {
    EXCEPTION_STATE.with(|c| f(c.get()))
}

// ---------------------------------------------------------------------------
// The landing pad's restore.
// ---------------------------------------------------------------------------

/// The first call of every landing pad, generated or the trap shim's:
/// `js_eh_pad_enter(llvm.stacksave(), handle_height)`. Pops, on every managed
/// stack, the entries pushed by frames the throw abandoned — those whose mark
/// lies below `sp`, the catching frame's stack pointer — and truncates the
/// runtime-handle stack to `handle_height`, the value the catching frame read
/// with [`js_eh_handle_height`] when it entered its handler scope (see
/// `marks`). Idempotent; runs no JS and allocates nothing.
///
/// A pad entered means the throw was caught, so the `throw_not_callable`
/// circuit breaker (#2780) resets here: valid JS that throws and catches a
/// non-callable many times must not trip the abort meant for runaway
/// *uncaught* throw loops.
#[no_mangle]
pub extern "C" fn js_eh_pad_enter(sp: *const u8, handle_height: u64) {
    #[cfg(debug_assertions)]
    with_exception_state(|s| unsafe { (*s).in_pad_enter = true });
    crate::closure::reset_throw_not_callable_counter();
    #[cfg(test)]
    if crate::native_payload::callback_sabotage("pad_enter") {
        #[cfg(debug_assertions)]
        with_exception_state(|s| unsafe { (*s).in_pad_enter = false });
        return;
    }
    marks::unwind_managed_stacks(sp as usize, handle_height as usize);
    #[cfg(debug_assertions)]
    with_exception_state(|s| unsafe { (*s).in_pad_enter = false });
}

/// The runtime-handle stack's height, read where a generated handler scope is
/// entered and in the trap shim before its `invoke`; the scope's pads pass it
/// back to [`js_eh_pad_enter`] (see `marks`). `u64` on
/// every target so the generated declaration has one shape.
#[no_mangle]
pub extern "C" fn js_eh_handle_height() -> u64 {
    crate::gc::runtime_handle_stack_len() as u64
}

// ---------------------------------------------------------------------------
// WASI handlers (setjmp transport).
// ---------------------------------------------------------------------------

/// Push a WASI handler and return its jmp_buf. `mark` is the handler frame's
/// stack pointer (`llvm.stacksave` in a generated `try`, [`frame_mark`] in a
/// runtime trap); a throw restores every managed stack to it, as a landing
/// pad at that frame would.
///
/// The buffer is armed by the C trampoline (`perry_sjlj_try`, via [`trap`]) or
/// by generated WASI code, NEVER by a raw `setjmp` call from Rust: rustc
/// cannot express `returns_twice` (#9305).
#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn js_try_push(mark: *const u8) -> *mut i32 {
    with_exception_state(|s| unsafe {
        let depth = (*s).handlers.len();
        (*s).handlers.push(WasiHandler {
            mark: mark as usize,
            handles: crate::gc::runtime_handle_stack_len(),
            shadow: crate::gc::frame_root_savepoint(),
            temp_roots: crate::gc::temp_root_len(),
            derived_super: crate::object::derived_super_binding_len(),
        });
        (*s).jump_buffer_ptr(depth)
    })
}

/// End a WASI `try` (pop its handler). Does NOT clear the exception.
#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn js_try_end() {
    with_exception_state(|s| unsafe {
        (*s).handlers.pop();
    });
}

// ---------------------------------------------------------------------------
// Runtime traps (ehfree 2b): a landing-pad shim on native, setjmp on WASI.
// ---------------------------------------------------------------------------

/// The body a runtime trap runs. `C-unwind`: a JS throw unwinds out of it
/// into the shim's landing pad, and a plain `"C"` boundary would plant
/// `panic_cannot_unwind` there under panic=unwind (see [`js_throw`]).
pub type TrapBody = unsafe extern "C-unwind" fn(*mut core::ffi::c_void);

// Declared `extern "C"` (nounwind) on purpose: the shim never unwinds out —
// its pad catches every Perry throw. A `"C-unwind"` foreign declaration would
// give each call an unwind edge; rustc then plants a terminate pad on it in
// the panic=abort runtime, and that function's LSDA (rust_eh_personality,
// listing only the shim call) makes every OTHER call site in it a terminate
// site, so a throw passing through such a frame failed phase 1 with
// `_URC_FATAL_PHASE1_ERROR` (gap test_gap_7911, run_combinator).
#[cfg(not(target_os = "wasi"))]
extern "C" {
    /// The landing-pad shim (`src/ffi/perry_trap.ll`, compiled by build.rs):
    /// `invoke body(ctx)` under a catch-all pad with Perry's personality,
    /// whose first call is [`js_eh_pad_enter`]. Returns 0 when `body`
    /// returned and 1 when a JS throw landed in the pad. Nothing is ever
    /// returned into twice, so Rust callers need no setjmp discipline (#9305).
    fn perry_trap_call(body: TrapBody, ctx: *mut core::ffi::c_void) -> core::ffi::c_int;
    /// The same shim for the worker base trap ([`run_worker_body`]); its pad
    /// returns 2. The personality accepts only this frame while
    /// [`hand_to_worker_base`] carries an exception to the base.
    fn perry_trap_call_base(body: TrapBody, ctx: *mut core::ffi::c_void) -> core::ffi::c_int;
}

#[cfg(target_os = "wasi")]
extern "C" {
    /// WASI setjmp trampoline (`src/ffi/perry_sjlj.c`): arms `env` in its own
    /// C frame and runs `body(ctx)`; returns 0, or the `longjmp` value (1)
    /// when a JS throw lands.
    fn perry_sjlj_try(
        env: *mut core::ffi::c_void,
        body: TrapBody,
        ctx: *mut core::ffi::c_void,
    ) -> core::ffi::c_int;
}

/// Function start of the worker base shim, for the personality's handoff
/// check (`crate::eh`, `crate::eh_windows`).
#[cfg(not(target_os = "wasi"))]
pub(crate) fn worker_base_shim_addr() -> usize {
    perry_trap_call_base as *const () as usize
}

/// True while [`hand_to_worker_base`] carries an exception to the worker's
/// base trap: the personality then refuses every handler but that one.
#[cfg(not(target_os = "wasi"))]
pub(crate) fn worker_base_handoff_pending() -> bool {
    with_exception_state(|s| unsafe { (*s).to_worker_base })
}

/// Run `body(ctx)` under a runtime trap. `true`: `body` returned. `false`: a
/// JS throw landed; the value is in the TLS exception slot, and every managed
/// stack is back where it was when the trap was entered.
///
/// Shadow-stack targets (and the runtime's own tests) also keep the shadow
/// frame depth here: compiled frames push shadow frames, and a shadow frame
/// is restored by the frame that holds its handle — for a trap, this frame.
#[inline]
unsafe fn trap_raw<const BASE: bool>(body: TrapBody, ctx: *mut core::ffi::c_void) -> bool {
    #[cfg(not(target_os = "wasi"))]
    {
        #[cfg(any(test, not(perry_native_stack_maps)))]
        let shadow = crate::gc::frame_root_savepoint();
        let rc = if BASE {
            perry_trap_call_base(body, ctx)
        } else {
            perry_trap_call(body, ctx)
        };
        #[cfg(any(test, not(perry_native_stack_maps)))]
        if rc != 0 {
            crate::gc::frame_root_restore(shadow);
        }
        rc == 0
    }
    #[cfg(target_os = "wasi")]
    {
        let env = js_try_push(frame_mark() as *const u8);
        let returned = perry_sjlj_try(env.cast(), body, ctx) == 0;
        js_try_end();
        returned
    }
}

/// Run `f` under a runtime trap.
///
/// `Some(r)`: `f` completed normally. `None`: a JS throw landed while `f` (or
/// JS it called into) was running. The thrown value is left in the TLS
/// exception slot (`js_get_exception`), not yet cleared.
///
/// A landing abandons the frames between the throw point and the shim
/// without running their cleanups (the runtime is panic=abort); the shim's
/// pad restores the runtime's own state ([`js_eh_pad_enter`]).
pub fn trap<R>(f: impl FnOnce() -> R) -> Option<R> {
    trap_with::<false, R, _>(f)
}

fn trap_with<const BASE: bool, R, F: FnOnce() -> R>(f: F) -> Option<R> {
    struct Ctx<F, R> {
        f: Option<F>,
        ret: Option<R>,
    }
    unsafe extern "C-unwind" fn invoke<F: FnOnce() -> R, R>(raw: *mut core::ffi::c_void) {
        // SAFETY: `raw` is the `&mut Ctx` passed below, alive for the whole
        // shim call; the shim invokes us exactly once.
        let ctx = unsafe { &mut *(raw as *mut Ctx<F, R>) };
        let f = ctx.f.take().expect("trap shim invoked body twice");
        ctx.ret = Some(f());
    }
    let mut ctx: Ctx<F, R> = Ctx {
        f: Some(f),
        ret: None,
    };
    // SAFETY: the shim's frame stays alive while `f` runs; `ctx` outlives it.
    let returned = unsafe {
        trap_raw::<BASE>(
            invoke::<F, R>,
            &mut ctx as *mut Ctx<F, R> as *mut core::ffi::c_void,
        )
    };
    if returned {
        Some(
            ctx.ret
                .take()
                .expect("trap shim returned without a body result"),
        )
    } else {
        None
    }
}

/// C entry for runtime traps in other crates (perry-ext-http; they declare it
/// plain `extern "C"`, see `perry_trap_call`): run `body(ctx)` under a trap.
/// Returns 0 when `body` returned and 1 when a JS throw landed; the thrown
/// value is then in the exception slot (`js_get_exception`), not yet cleared.
///
/// # Safety
/// `body` must be safe to call with `ctx`.
#[no_mangle]
pub unsafe extern "C" fn js_trap_call(body: TrapBody, ctx: *mut core::ffi::c_void) -> i32 {
    i32::from(!trap_raw::<false>(body, ctx))
}

/// Take and clear the pending exception after a trap landed.
#[inline]
fn take_exception() -> f64 {
    let err = js_get_exception();
    js_clear_exception();
    err
}

/// Run `f` under a trap. `Ok(value)` on a normal return; `Err(exception_bits)`
/// — with the TLS exception slot cleared — if `f` threw.
pub fn catch_js_throw<R>(f: impl FnOnce() -> R) -> Result<R, f64> {
    match trap(f) {
        Some(r) => Ok(r),
        None => Err(take_exception()),
    }
}

/// The fixed generator boundary calls one known two-argument compiled body.
/// Pass the rooted invocation directly instead of transporting FnOnce/Option
/// through another Rust callback frame on every yield.
#[inline(never)]
pub(crate) unsafe fn catch_generator_resume(
    state: &crate::gc::RuntimeHandle<'_>,
    op: f64,
    arg: f64,
) -> Result<f64, f64> {
    struct Invocation<'a, 'scope> {
        state: &'a crate::gc::RuntimeHandle<'scope>,
        arg: f64,
        op: f64,
        result: f64,
    }
    unsafe extern "C-unwind" fn invoke(raw: *mut core::ffi::c_void) {
        let ctx = &mut *raw.cast::<Invocation<'_, '_>>();
        // next's argument is already published in the rooted fixed frame.
        // Read it there after any barrier work, directly into the body ABI.
        // Abrupt arguments cross only non-collecting exception-stack setup.
        let result = ctx
            .state
            .with_const_ptr::<crate::closure::ClosureHeader, _>(|p| {
                let arg = if ctx.op == 0.0 {
                    f64::from_bits(*p.add(1).cast::<u64>().add(2))
                } else {
                    ctx.arg
                };
                if (*(*p).info).params == 1 {
                    return crate::closure::body_call::js_body_call!(
                        (*(*p).info).code,
                        p,
                        crate::closure::plain_call_receiver(),
                        arg
                    );
                }
                crate::closure::body_call::js_body_call!(
                    (*(*p).info).code,
                    p,
                    crate::closure::plain_call_receiver(),
                    ctx.op,
                    arg
                )
            });
        // GC_STORE_AUDIT(STACK): trampoline result read before any further call.
        ctx.result = result;
    }
    let mut ctx = Invocation {
        state,
        arg,
        op,
        result: 0.0,
    };
    if trap_raw::<false>(invoke, (&raw mut ctx).cast()) {
        Ok(ctx.result)
    } else {
        Err(take_exception())
    }
}

crate::perry_thread_local! {
    /// True while this thread's body runs under [`run_worker_body`]'s base
    /// trap.
    static IN_WORKER_BASE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run a worker thread's whole body under one base trap. Everything the
/// worker leaves unhandled lands here: a throw no `try` catches, an uncaught
/// exception no `uncaughtException` listener takes (from a timer or a
/// listener), and an unhandled rejection. `Err(value)` is that exception. The
/// caller ends the worker; the process keeps running, as in Node.
pub fn run_worker_body<R>(f: impl FnOnce() -> R) -> Result<R, f64> {
    let outer = IN_WORKER_BASE.with(|d| d.replace(true));
    let outcome = trap_with::<true, R, _>(f);
    IN_WORKER_BASE.with(|d| d.set(outer));
    // A handoff ends here, at the base pad.
    #[cfg(not(target_os = "wasi"))]
    with_exception_state(|s| unsafe { (*s).to_worker_base = false });
    match outcome {
        Some(r) => Ok(r),
        None => Err(take_exception()),
    }
}

/// On a worker thread, hand an uncaught exception to the worker's base trap
/// (see [`run_worker_body`]); this does not return. On any other thread it
/// returns and the caller ends the process.
pub(crate) fn hand_to_worker_base(value: f64) {
    if !IN_WORKER_BASE.with(std::cell::Cell::get) {
        return;
    }
    #[cfg(not(target_os = "wasi"))]
    {
        // Like `js_throw`, but straight to the base trap: the handlers between
        // here and there belong to drains (microtasks, timers, rejection
        // listeners) that would report the exception again or swallow it, or
        // to JS `try`s that must not see it. The flag makes the personality
        // refuse every pad but the base shim's (`perry_trap_call_base`), whose
        // pad restores the managed stacks like any other.
        with_exception_state(|s| unsafe {
            crate::gc::runtime_store_root_nanbox_f64_raw_slot(
                &raw mut (*s).current_exception,
                value,
            );
            (*s).has_exception = true;
            (*s).in_finally = false;
            (*s).to_worker_base = true;
        });
        // The single-phase transport applies the personality's handoff rule
        // (only the base shim's pad), so it carries this throw too; it falls
        // back to the system unwinder exactly as `js_throw` does.
        crate::eh::try_fast_transport();
        let reason = crate::eh::raise_perry_exception();
        eprintln!("perry: FATAL: worker base handoff found no base trap (reason={reason})");
        std::process::abort();
    }
}

/// End this thread's JS for an uncaught exception: a worker's base trap
/// takes it, and anywhere else it is printed and the process exits with 1.
pub(crate) fn exit_on_uncaught(value: f64) -> ! {
    hand_to_worker_base(value);
    print_uncaught(value);
    crate::process::exit_after_current_thread_collection_teardown(1)
}

/// Invoke `f` — which may call into user JS and `js_throw` — inside a trap,
/// catching any JS exception. Returns `Ok(value)` on a normal return, or
/// `Err(exception_bits)` if `f` threw. The catch is the trap shim's landing
/// pad (see [`trap`]), whose frame stays alive while `f` runs, so the throw
/// unwinds only up to there — NOT past the Rust caller's frame.
///
/// Runtime helpers that drive user JS from a Rust-owned microtask/timer
/// context (e.g. a Web Streams `pull` callback) use this so a throwing
/// callback errors the relevant object per spec instead of unwinding past
/// their frames — skipping cleanup and corrupting state. Mirrors
/// `combinators::combinator_catch_js`.
pub fn js_call_catching(f: impl FnOnce() -> f64) -> Result<f64, f64> {
    catch_js_throw(f)
}

/// The uncaught path, on an intact stack: Node's `exit` listeners run first
/// (with code 1, BEFORE the report, #9403 — verified against node 26.5.1
/// with the two streams separated), then the report, then exit with the
/// status the listeners may have rewritten via `process.exitCode`.
fn exit_uncaught(value: f64) -> ! {
    let status = crate::process::run_process_exit_sequence(Some(1));
    print_uncaught(value);
    emit_uncaught_backtrace();
    std::process::exit(status);
}

/// Throw an exception with the given value
///
/// `C-unwind` is required for the landingpad transport: when the fast walker
/// declines (notably across separately loaded provider/app images), the system
/// unwinder must be allowed to leave this Rust ABI boundary and reach the
/// generated frame's handler. Plain `extern "C"` installs an aborting guard at
/// the function boundary and turns a catchable JS throw into
/// `panic_cannot_unwind` before the landing pad can run.
#[no_mangle]
pub extern "C-unwind" fn js_throw(value: f64) -> ! {
    // The throws premise of the generated GC-effects table: a collection never
    // unwinds. Throw path only (see `forbid_throw_in_collection`).
    crate::gc::allocation_pacing::forbid_throw_in_collection();
    with_exception_state(|s| unsafe {
        #[cfg(debug_assertions)]
        if (*s).in_pad_enter {
            eprintln!("perry: FATAL: js_throw while a landing pad restores the managed stacks");
            std::process::abort();
        }
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(&raw mut (*s).current_exception, value);
        (*s).has_exception = true;
        if (*s).in_finally {
            eprintln!("Cannot throw during finally block");
            std::process::abort();
        }
    });
    // WASI handlers (generated and runtime traps) use libsetjmp over
    // standardized wasm EH. Restore to the handler frame's mark, exactly as a
    // landing pad at that frame would, then jump.
    #[cfg(target_os = "wasi")]
    {
        let jb = with_exception_state(|s| unsafe {
            let depth = (*s).handlers.len().checked_sub(1)?;
            Some(((&(*s).handlers)[depth], (*s).jump_buffer_ptr(depth)))
        });
        let Some((handler, jb)) = jb else {
            exit_uncaught(value)
        };
        crate::closure::reset_throw_not_callable_counter();
        marks::unwind_managed_stacks(handler.mark, handler.handles);
        crate::gc::frame_root_restore(handler.shadow);
        crate::gc::temp_roots_truncate_to(handler.temp_roots);
        crate::object::derived_super_binding_truncate(handler.derived_super);
        unsafe { longjmp(jb, 1) }
    }
    #[cfg(not(target_os = "wasi"))]
    {
        // First throw of the process: prove the unwinder can step runtime
        // frames. A runtime built without forced unwind tables would end
        // every cross-helper throw's search early, and that must not read as
        // an uncaught exception. Windows needs no check — MSVC x64 unwind
        // tables are mandatory for all functions.
        #[cfg(not(windows))]
        crate::eh::verify_unwind_tables_once();
        #[cfg(windows)]
        crate::eh::install_uncaught_filter_once();
        // Single-phase transport (every native target): walk to the handler
        // frame and install its register context directly. Never returns on
        // success. Declines (no handler, undecodable frame, disabled, or
        // verification mode) fall through to the two-phase raise below.
        crate::eh::try_fast_transport();
        let reason = crate::eh::raise_perry_exception();
        // Itanium: phase 1 found no pad anywhere, and the raise returned
        // without unwinding. The stack is intact, so exit listeners and the
        // backtrace behave as before. Classified by the personality's
        // verdicts, not by `reason`: some libgcc builds return garbage from
        // `_Unwind_RaiseException` (see `eh::raise_found_no_handler`).
        // (Windows reaches its uncaught path in the unhandled-exception
        // filter, before RaiseException returns.)
        #[cfg(not(windows))]
        if crate::eh::raise_found_no_handler() {
            crate::eh::diagnose_end_of_stack(reason);
            exit_uncaught(value)
        }
        eprintln!(
            "perry: FATAL: exception transport failed (reason={reason}). The runtime or an \
             intermediate object was built without unwind tables, or an unwinder returned \
             a throw it had accepted."
        );
        print_uncaught(value);
        emit_uncaught_backtrace();
        std::process::abort();
    }
}

/// The Windows uncaught path: the unhandled-exception filter
/// (`crate::eh_windows`) calls this for a Perry throw no frame accepted,
/// with the stack still intact.
#[cfg(windows)]
pub(crate) fn exit_uncaught_pending() -> ! {
    exit_uncaught(js_get_exception())
}

/// Get the current exception value
#[no_mangle]
pub extern "C" fn js_get_exception() -> f64 {
    with_exception_state(|s| unsafe { (*s).current_exception })
}

/// Check if there's an active exception
#[no_mangle]
pub extern "C" fn js_has_exception() -> i32 {
    with_exception_state(|s| unsafe {
        if (*s).has_exception {
            1
        } else {
            0
        }
    })
}

/// Clear the current exception
#[no_mangle]
pub extern "C" fn js_clear_exception() {
    with_exception_state(|s| unsafe {
        (*s).has_exception = false;
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(&raw mut (*s).current_exception, 0.0);
    });
}

/// A `catch` clause's entry: take the pending exception and clear it. On
/// WASI it first ends the `try` (pops its setjmp handler); a landing pad has
/// nothing to pop.
#[no_mangle]
pub extern "C" fn js_catch_enter() -> f64 {
    #[cfg(target_os = "wasi")]
    js_try_end();
    take_exception()
}

/// Mark entering a finally block
#[no_mangle]
pub extern "C" fn js_enter_finally() {
    with_exception_state(|s| unsafe {
        (*s).in_finally = true;
    });
}

/// Mark leaving a finally block
#[no_mangle]
pub extern "C" fn js_leave_finally() {
    with_exception_state(|s| unsafe {
        (*s).in_finally = false;
    });
}

/// Read a StringHeader into an owned Rust String (empty on null/garbage).
pub(crate) unsafe fn string_header_to_string(ptr: *const crate::string::StringHeader) -> String {
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return String::new();
    }
    let len = (*ptr).byte_len as usize;
    // Guard against corrupt lengths — StringHeader lengths above ~1GB
    // indicate a stale/bogus pointer (e.g. misread via a wrong tag).
    if len > 1 << 30 {
        return String::new();
    }
    let bytes_ptr = (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
    std::str::from_utf8(std::slice::from_raw_parts(bytes_ptr, len))
        .unwrap_or("?")
        .to_string()
}

/// Emit a symbolicated native backtrace for an UNCAUGHT throw, behind
/// `PERRY_UNCAUGHT_BACKTRACE=1` (#7803 tooling).
///
/// # Why this exists
///
/// A #7154-class rooting bug surfaces as an uncaught `TypeError` in a function
/// that is nowhere near the code that lost the value, and the JS-level `stack`
/// this path already prints reads `at <anonymous>` — one frame, no name. The
/// native stack, in contrast, names every compiled JS frame: `--debug-symbols`
/// keeps 1726 `_perry_fn_*` / `_perry_closure_*` symbols in the corpus binary,
/// so `backtrace_symbols_fd` resolves the whole chain through `dladdr`.
///
/// The obvious alternative — run the failing binary under a debugger and break
/// on the throw helper — was tried first for #7803 and is NOT equivalent: the
/// failure is intermittent, and under `lldb` the same seeds that fail natively
/// pass. An instrument that only works when the bug does not reproduce is not
/// an instrument. This one runs in the ordinary process, so it observes the
/// run that actually fails.
///
/// Off by default and read once per uncaught throw, i.e. at most once per
/// process, on a path that is already about to `exit(1)`.
///
/// Parsed by VALUE, not by presence: `PERRY_GC_DIAG` was `var_os(..).is_some()`
/// for long enough that `PERRY_GC_DIAG=0` ENABLED diagnostics and silently
/// collapsed one arm of an A/B (ZOD-NOTES §3, fixed in #7993). A new knob does
/// not get to repeat that.
fn emit_uncaught_backtrace() {
    let on = matches!(
        std::env::var("PERRY_UNCAUGHT_BACKTRACE").ok().as_deref(),
        Some("1") | Some("on") | Some("true")
    );
    if !on {
        return;
    }
    // glibc-only pair; musl has no `backtrace`, so this block must not be
    // selected there (E0425 at release time on the musl leg).
    #[cfg(all(
        unix,
        any(target_os = "macos", all(target_os = "linux", target_env = "gnu"))
    ))]
    {
        const MAX_FRAMES: usize = 96;
        let mut frames = [std::ptr::null_mut::<libc::c_void>(); MAX_FRAMES];
        eprintln!("--- native backtrace at the uncaught throw ---");
        // SAFETY: `backtrace` / `backtrace_symbols_fd` are the async-signal-safe
        // pair — `_fd` writes to the descriptor directly and does not allocate.
        // Same call shape as `arena::quarantine::emit_native_backtrace`.
        unsafe {
            let n = libc::backtrace(frames.as_mut_ptr(), MAX_FRAMES as libc::c_int);
            if n > 0 {
                libc::backtrace_symbols_fd(frames.as_ptr(), n, 2);
            }
        }
        eprintln!("--- end native backtrace ---");
    }
}

/// Render the line `print_uncaught` emits for a native `ErrorHeader`.
///
/// Extracted so the ORDER of the reads is testable, which is the whole point
/// of this function existing (#9486). Node formats an uncaught throw as
/// `<Name>: <message>` followed by the frames and no `Uncaught exception:`
/// prefix (#616), and `Error.stack` already starts with that head, so the
/// stack IS the report; the head is rebuilt only when there is no stack.
///
/// # Every read off `eh` happens BEFORE the stack is materialised
///
/// `js_error_get_stack` allocates: since #9486 the string is built on first
/// read, not at construction. That allocation is a GC point, and an
/// evacuating scavenge moves the error out from under `eh` — a plain Rust
/// local, which perry's collector neither scans nor pins, so afterwards it
/// names from-space. Reading `(*eh).message` there to resolve the `ERR_*`
/// code is an unsound read whose result is whatever the collector left in the
/// vacated cell, and the address it yields is then used as a side-table key.
/// Reading everything first costs nothing and needs no root, which is why
/// this is an ordering fix rather than a `RuntimeHandleScope`.
///
/// (Before #9486 the stack came from a plain field load, so no GC point
/// existed between the reads at all.)
pub(crate) unsafe fn uncaught_native_error_report(eh: *mut crate::error::ErrorHeader) -> String {
    let name_str = string_header_to_string((*eh).name);
    let msg_str = string_header_to_string((*eh).message);
    let code = crate::node_submodules::error_code_for_error(eh);
    // Nothing may read `eh` past this line.
    let stack_str = string_header_to_string(crate::error::js_error_get_stack(eh));

    let name_display = if name_str.is_empty() {
        "Error"
    } else {
        &name_str
    };
    if !stack_str.is_empty() {
        match code {
            Some(code) => {
                let frames = stack_str
                    .split_once('\n')
                    .map(|(_, frames)| format!("\n{frames}"))
                    .unwrap_or_default();
                format!("{name_display} [{code}]: {msg_str}{frames}")
            }
            None => stack_str,
        }
    } else if msg_str.is_empty() {
        name_display.to_string()
    } else {
        format!("{name_display}: {msg_str}")
    }
}

/// Best-effort display of a thrown value for uncaught-exception reporting.
/// Matches Node semantics roughly: Errors print `name: message` + stack,
/// regular objects probe for `.message`/`.stack`, everything else goes
/// through the generic `js_jsvalue_to_string` (which handles strings,
/// numbers, booleans, arrays, user `[Symbol.toPrimitive]`, etc.).
pub(crate) fn print_uncaught(value: f64) {
    let bits = value.to_bits();
    let top16 = bits >> 48;

    if top16 == 0x7FFD {
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as usize;
        if ptr >= 0x10000 {
            // #8113: both discriminators come from the GC header / ShapeId
            // descriptor now. Offset 0 is `class_id`, so the old raw
            // `*(ptr as *const u32)` read would classify the second class a
            // program declares (`class_id == 2 == OBJECT_TYPE_ERROR`) as an
            // Error and print `name`/`message`/`stack` out of its field slots.
            if unsafe { crate::error::ptr_is_native_error(ptr) } {
                eprintln!("{}", unsafe {
                    uncaught_native_error_report(ptr as *mut crate::error::ErrorHeader)
                });
                return;
            }
            if unsafe {
                crate::object::object_is_regular(ptr as *const crate::object::ObjectHeader)
            } {
                // Probe for `.message` and `.stack` properties the way
                // Node does for thrown non-Error objects. Users commonly
                // throw custom error shapes like `{ message, stack }` or
                // user-class instances that carry those fields.
                let msg_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
                let stack_key = crate::string::js_string_from_bytes(b"stack".as_ptr(), 5);
                let msg_val = crate::object::js_object_get_field_by_name_f64(
                    ptr as *const crate::object::ObjectHeader,
                    msg_key as *const crate::string::StringHeader,
                );
                let stack_val = crate::object::js_object_get_field_by_name_f64(
                    ptr as *const crate::object::ObjectHeader,
                    stack_key as *const crate::string::StringHeader,
                );
                let msg_str_ptr = crate::value::js_jsvalue_to_string(msg_val);
                let msg_str = unsafe { string_header_to_string(msg_str_ptr) };
                if !msg_str.is_empty() && msg_str != "undefined" {
                    eprintln!("Uncaught exception: {}", msg_str);
                } else {
                    let obj_str_ptr = crate::value::js_jsvalue_to_string(value);
                    let obj_str = unsafe { string_header_to_string(obj_str_ptr) };
                    if obj_str.is_empty() || obj_str == "[object Object]" {
                        eprintln!("Uncaught exception: [object] (bits=0x{:016X})", bits);
                    } else {
                        eprintln!("Uncaught exception: {}", obj_str);
                    }
                }
                let stack_str_ptr = crate::value::js_jsvalue_to_string(stack_val);
                let stack_str = unsafe { string_header_to_string(stack_str_ptr) };
                if !stack_str.is_empty() && stack_str != "undefined" {
                    eprintln!("{}", stack_str);
                }
                return;
            }
            // Fall through to generic stringify for arrays, promises,
            // bigints, maps, etc. — js_jsvalue_to_string handles them all.
        }
    }

    let s_ptr = crate::value::js_jsvalue_to_string(value);
    let s = unsafe { string_header_to_string(s_ptr) };
    if s.is_empty() {
        eprintln!("Uncaught exception: (bits=0x{:016X})", bits);
    } else {
        eprintln!("Uncaught exception: {}", s);
    }
}

/// GC root scanner: mark the current exception value
pub fn scan_exception_roots(mark: &mut dyn FnMut(f64)) {
    let mut visitor = crate::gc::RuntimeRootVisitor::for_copy(mark);
    scan_exception_roots_mut(&mut visitor);
}

pub fn scan_exception_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    with_exception_state(|s| unsafe {
        if (*s).has_exception {
            visitor.visit_nanbox_f64_raw_slot(&raw mut (*s).current_exception);
        }
    });
}

#[cfg(test)]
pub(crate) fn test_set_exception(value: f64) {
    with_exception_state(|s| unsafe {
        crate::gc::runtime_store_root_nanbox_f64_raw_slot(&raw mut (*s).current_exception, value);
        (*s).has_exception = true;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gc::{
        js_shadow_frame_pop, js_shadow_frame_push, js_shadow_slot_set, shadow_stack_depth,
        RuntimeHandleScope,
    };

    /// A trap restores the shadow stack it was entered with: the frames
    /// pushed by the abandoned callees are dropped before any later GC scans
    /// roots (#1830). All assertions are relative to the entry state, so
    /// this is robust under `--test-threads=1` (shared TLS).
    #[test]
    fn a_trap_landing_restores_shadow_stack_across_unwound_frames() {
        let base_depth = shadow_stack_depth();
        let run_frame = js_shadow_frame_push(1);
        js_shadow_slot_set(0, 0x7FFD_0000_0000_0001);
        let depth_at_try = shadow_stack_depth();
        let r: Result<(), f64> = catch_js_throw(|| {
            let _f1 = js_shadow_frame_push(1);
            js_shadow_slot_set(0, 0x7FFD_0000_0000_00A1);
            let _f2 = js_shadow_frame_push(2);
            js_shadow_slot_set(0, 0x7FFD_0000_0000_00B1);
            assert_eq!(shadow_stack_depth(), depth_at_try + 2);
            js_throw(1.0)
        });
        assert_eq!(r, Err(1.0));
        assert_eq!(
            shadow_stack_depth(),
            depth_at_try,
            "the landing dropped the orphaned callee frames"
        );
        js_shadow_frame_pop(run_frame);
        assert_eq!(shadow_stack_depth(), base_depth);
    }

    /// The pad's restore rolls the runtime handle stack back to the scopes
    /// that are still live: the trap frame's own scope keeps its handle, the
    /// abandoned frames' handles are gone.
    #[test]
    fn a_pad_restores_runtime_handles_across_unwound_frames() {
        #[inline(never)]
        fn abandoned() -> ! {
            let scope = RuntimeHandleScope::new();
            let _orphaned = scope.root_nanbox_f64(f64::from_bits(0x7FFD_0000_0000_00A1));
            std::mem::forget(scope);
            js_throw(3.0)
        }
        let base_handles = RuntimeHandleScope::active_len_for_tests();
        let scope = RuntimeHandleScope::new();
        let _live = scope.root_nanbox_f64(f64::from_bits(0x7FFD_0000_0000_00B1));
        let r: Result<(), f64> = catch_js_throw(|| abandoned());
        assert_eq!(r, Err(3.0));
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), base_handles + 1);
        drop(scope);
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), base_handles);
    }

    /// PR #10564 review finding: a bare save/call/restore pair — no RAII
    /// guard — around a call that throws (the Temporal/Intl subclass
    /// `super()` bridges). The restore statement textually follows the call,
    /// so no transport ever reaches it once the call throws; the sites root the
    /// previous value as a new.target save and the pad restores it.
    #[test]
    fn a_bare_save_call_restore_site_is_made_exception_safe_by_the_pad() {
        let base_nt = crate::object::js_new_target_get().to_bits();
        let outcome: Result<(), f64> = catch_js_throw(|| {
            let inner: Result<(), f64> = catch_js_throw(|| {
                let scope = crate::gc::RuntimeHandleScope::new();
                let _save = scope.root_new_target_save(crate::object::js_new_target_set(22.0));
                js_throw(99.0)
            });
            assert_eq!(inner, Err(99.0));
            assert_eq!(
                crate::object::js_new_target_get().to_bits(),
                base_nt,
                "the pad of the trap around the bare site must have restored new.target"
            );
        });
        assert_eq!(outcome, Ok(()));
        assert_eq!(crate::object::js_new_target_get().to_bits(), base_nt);
    }

    /// The trap transport round-trips a throw: a real `js_throw` raises from
    /// inside the protected body and lands in the shim's pad (setjmp
    /// trampoline on WASI); the Rust caller observes a single-return call and
    /// `None`, and can trap again at once.
    #[test]
    fn trap_catches_a_real_throw_and_traps_again() {
        assert_eq!(trap(|| 7), Some(7));
        let landed = trap(|| -> i32 { js_throw(42.0) });
        assert!(landed.is_none(), "throw must land in the trap");
        assert_eq!(js_get_exception(), 42.0);
        js_clear_exception();
        assert_eq!(trap(|| 8), Some(8));
    }

    /// `catch_js_throw`: Err with the TLS exception cleared.
    #[test]
    fn catch_js_throw_err_clears_exception() {
        assert_eq!(catch_js_throw(|| 3usize), Ok(3));
        let r: Result<usize, f64> = catch_js_throw(|| js_throw(7.5));
        assert_eq!(r, Err(7.5));
        assert_eq!(js_has_exception(), 0);
        assert_eq!(catch_js_throw(|| 1u8), Ok(1));
    }

    /// Nested traps land innermost first; the outer trap still works after
    /// the inner one returned.
    #[test]
    fn nested_trampoline_arms_unwind_innermost_first() {
        let outcome = catch_js_throw(|| {
            let inner: Result<u8, f64> = catch_js_throw(|| js_throw(1.0));
            assert_eq!(inner, Err(1.0));
            js_throw(2.0)
        });
        assert_eq!(outcome, Err(2.0));
    }

    /// A throw from deep inside a trap's body (hundreds of Rust frames, each
    /// holding a runtime handle) lands in that trap's pad, and the pad rolls
    /// the handle stack back to the trap's entry.
    #[test]
    fn a_throw_at_depth_lands_in_the_trap_pad() {
        #[inline(never)]
        fn descend(n: usize, scope: &RuntimeHandleScope) -> usize {
            let _h = scope.root_nanbox_f64(f64::from_bits(0x7FFD_0000_0000_0000 | n as u64));
            if n == 0 {
                js_throw(13.0)
            }
            std::hint::black_box(descend(n - 1, scope)) + 1
        }
        let base_handles = RuntimeHandleScope::active_len_for_tests();
        let r = catch_js_throw(|| {
            let scope = RuntimeHandleScope::new();
            descend(400, &scope)
        });
        assert_eq!(r, Err(13.0));
        assert_eq!(RuntimeHandleScope::active_len_for_tests(), base_handles);
    }

    /// The C entry other crates use: 0 when the body returns, 1 with the
    /// exception left in the slot when it throws.
    #[test]
    fn js_trap_call_reports_both_outcomes() {
        unsafe extern "C-unwind" fn ok(ctx: *mut core::ffi::c_void) {
            *ctx.cast::<i32>() = 9;
        }
        unsafe extern "C-unwind" fn throws(_ctx: *mut core::ffi::c_void) {
            js_throw(6.5)
        }
        let mut out = 0i32;
        assert_eq!(unsafe { js_trap_call(ok, (&raw mut out).cast()) }, 0);
        assert_eq!(out, 9);
        assert_eq!(unsafe { js_trap_call(throws, std::ptr::null_mut()) }, 1);
        assert_eq!(js_get_exception(), 6.5);
        js_clear_exception();
    }

    /// `hand_to_worker_base` passes every handler between the throw and the
    /// worker's base trap, runtime traps included, and lands at the base.
    #[test]
    #[cfg(not(target_os = "wasi"))]
    fn worker_base_handoff_skips_inner_traps() {
        let mut inner_landed = false;
        let r = run_worker_body(|| {
            let inner = catch_js_throw(|| {
                let deeper: Result<u8, f64> = catch_js_throw(|| {
                    hand_to_worker_base(21.0);
                    0
                });
                // Never reached: the handoff skips this trap's pad.
                inner_landed = deeper.is_err();
                0u8
            });
            inner_landed |= inner.is_err();
            1u8
        });
        assert_eq!(r, Err(21.0));
        assert!(!inner_landed);
        assert!(!worker_base_handoff_pending());
        // An ordinary throw afterwards lands in the innermost trap again.
        assert_eq!(catch_js_throw(|| -> u8 { js_throw(2.0) }), Err(2.0));
    }

    /// The owned walker recognizes the shim as a Perry handler, so a
    /// throw into a runtime trap takes the single-phase transport instead of
    /// falling back (or worse, skipping the shim for an outer pad).
    #[test]
    #[cfg(any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        ),
        all(target_os = "windows", target_arch = "x86_64")
    ))]
    fn the_fast_walker_carries_a_throw_into_a_trap() {
        #[cfg(target_os = "windows")]
        use crate::eh as transport;
        #[cfg(not(target_os = "windows"))]
        use crate::eh_walker as transport;
        use std::sync::atomic::Ordering::Relaxed;
        // Warm the walker's caches and images once.
        let _ = catch_js_throw(|| -> u8 { js_throw(0.0) });
        let fast = transport::FAST_TRANSPORTS.load(Relaxed);
        let falls = transport::FALLBACKS.load(Relaxed);
        assert_eq!(catch_js_throw(|| -> u8 { js_throw(1.0) }), Err(1.0));
        if std::env::var_os("PERRY_EH_WALKER").is_none() {
            assert_eq!(transport::FALLBACKS.load(Relaxed), falls);
            assert!(transport::FAST_TRANSPORTS.load(Relaxed) > fast);
        }
    }

    /// #9486: the extracted uncaught report carries the `ERR_*` code and the
    /// frames, in the shape `print_uncaught` emits.
    ///
    /// This function exists as a separate item so the ORDER of its reads is
    /// reviewable: `js_error_get_stack` allocates now, so every read off the
    /// raw `eh` has to happen before it. That ordering is held by the
    /// function's structure and its doc comment, NOT by this test — the
    /// harness can force a collection at a point of its own choosing
    /// (`gc_collect_minor`) but cannot schedule one inside
    /// `js_string_from_bytes`, and a test that cannot create the racing
    /// collection cannot assert the race is handled.
    ///
    /// What this does pin is the report itself: the code branch, the frame
    /// tail, and the `<Name> [<code>]: <message>` head.
    #[test]
    fn the_uncaught_report_carries_the_error_code_and_the_frames() {
        unsafe {
            let msg = crate::string::js_string_from_bytes(b"boom".as_ptr(), 4);
            crate::node_submodules::register_error_code_pub(msg, "ERR_TEST_9486");
            let err = crate::error::js_error_new_with_message(msg);

            let report = uncaught_native_error_report(err);

            let (head, frames) = report
                .split_once('\n')
                .unwrap_or_else(|| panic!("the report must carry frames; got {report:?}"));
            assert_eq!(
                head, "Error [ERR_TEST_9486]: boom",
                "node's head for a coded error is `<Name> [<code>]: <message>`"
            );
            assert!(
                frames.contains("    at "),
                "the frame tail must survive the code branch; got {frames:?}"
            );
        }
    }
}

#[cfg(test)]
mod catch_enter_tests {
    use super::*;

    /// `js_catch_enter` answers the pending exception and clears it.
    #[test]
    fn catch_enter_takes_and_clears_the_exception() {
        test_set_exception(42.5);
        assert_eq!(js_has_exception(), 1);
        assert_eq!(js_catch_enter(), 42.5);
        assert_eq!(
            js_has_exception(),
            0,
            "the catch entry clears the exception"
        );
        assert_eq!(js_get_exception(), 0.0);
    }
}
