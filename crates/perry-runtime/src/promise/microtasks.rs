//! The microtask runner — `js_promise_run_microtasks` — and the
//! result-propagation helper it uses. See `super` for the task queue
//! and Promise state types.

use super::*;

thread_local! {
    /// Promise currently being dispatched by the microtask runner after its
    /// task has been popped from TASK_QUEUE. While user callbacks run this is
    /// the mutable root that lets copied-minor rewrite the promise pointer
    /// before the runner reads `.next` for settlement or exception routing.
    pub(super) static CURRENT_MICROTASK_PROMISE: std::cell::Cell<*mut Promise>
        = const { std::cell::Cell::new(std::ptr::null_mut()) };

    /// Active callback/value/next tuple for a popped microtask. Task queue
    /// entries stop being roots as soon as they are popped, but callback
    /// dispatch can run arbitrary JS and GC before the runner settles `next`.
    pub(super) static CURRENT_MICROTASK_CALLBACK: std::cell::Cell<ClosurePtr>
        = const { std::cell::Cell::new(std::ptr::null()) };
    pub(super) static CURRENT_MICROTASK_VALUE: std::cell::Cell<f64>
        = const { std::cell::Cell::new(0.0) };
    pub(super) static CURRENT_MICROTASK_NEXT: std::cell::Cell<*mut Promise>
        = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

#[no_mangle]
pub extern "C" fn js_promise_run_microtasks() -> i32 {
    mt_profile_register();
    let mut ran = 0;

    ran += crate::async_hooks::drain_gc_destroy_queue();

    // First, tick timers to resolve any expired timer promises
    ran += crate::timer::js_timer_tick();

    // Process callback timers (setTimeout with callbacks)
    ran += crate::timer::js_callback_timer_tick();

    // Process interval timers (setInterval)
    ran += crate::timer::js_interval_timer_tick();

    // Process any scheduled resolutions (simulates async completions)
    ran += super::combinators::process_scheduled_resolves();

    // Process pending thread results (from perry/thread spawn)
    ran += crate::thread::js_thread_process_pending();

    // Then process the task queue.
    //
    // ── Exception trap (Issue #...): install ONE setjmp for the WHOLE
    // loop body, instead of a fresh setjmp per microtask. The previous
    // shape paid setjmp+js_try_push/end every microtask just so that a
    // `throw` from a callback could be re-routed to reject the chained
    // `next` promise. setjmp+longjmp on aarch64 saves ~16 callee-saved
    // x-regs and ~8 d-regs per call — that's ~25 ns per microtask, and
    // an async benchmark with 200k microtasks pays ~5 ms in setjmp cost
    // alone. The single outer setjmp captures the same "throw out of a
    // microtask body" case (since `js_throw` longjmps to the most recent
    // try block; if no user try is in scope, this one is it). When the
    // longjmp lands, we read the current promise context out of a
    // thread-local set just before invoking the callback, reject its
    // `next`, and continue the loop.
    //
    // ── macOS/BSD: use `_setjmp` (no signal-mask save) ────────────
    // On Apple platforms the C `setjmp(3)` saves the signal mask via a
    // `sigprocmask` system call AND saves the alt-signal-stack via
    // `__sigaltstack`. Profiling `promise_all_chains` showed those two
    // syscalls accounted for ~43% of CPU time even though `setjmp` is
    // called once per `run_microtasks` drain — each kernel-mode round
    // trip is ~25 μs because macOS arm64 uses BSD-style "save signal
    // state for siglongjmp" semantics. Perry never `siglongjmp`s out
    // of a signal handler — `js_throw` runs in normal user context, so
    // the signal mask doesn't need to be saved/restored on
    // setjmp/longjmp pairs. POSIX's `_setjmp` / `_longjmp` are exactly
    // that: setjmp/longjmp without the sigprocmask round-trip.
    //
    // On Linux glibc the C `setjmp` already doesn't save the signal
    // mask (POSIX leaves it implementation-defined; glibc opted for
    // the fast path), so the `setjmp` extern there is fine. Other
    // BSDs (FreeBSD, NetBSD, OpenBSD) match macOS — they too benefit
    // from `_setjmp`. We gate on `target_vendor = "apple"` for now
    // since that's where we've measured the win.
    // `setjmp` lives in `crate::ffi::setjmp` — one canonical extern
    // declaration shared with `gc.rs` (issue #856). The libc-matching
    // signature is `unsafe extern "C" fn(*mut c_int) -> c_int`; on
    // Apple it links to the fast `_setjmp(3)` variant, on glibc Linux
    // to plain `setjmp(3)` which already skips the signal-mask save.
    use crate::ffi::setjmp::setjmp;

    let trap_buf = crate::exception::js_try_push();
    // SAFETY: The setjmp call must remain in this stack frame; we
    // longjmp to it from `js_throw` only while this frame is still
    // alive (inside the loop below). The cast `*mut i32 -> *mut c_int`
    // is a no-op on every Perry-supported target (c_int is i32
    // everywhere), but it spells the intent at the FFI boundary so
    // the shared declaration in `ffi::setjmp` stays the single source
    // of truth for libc's signature.
    let jumped = unsafe { setjmp(trap_buf as *mut std::os::raw::c_int) };
    if jumped != 0 {
        restore_all_microtask_contexts();
        crate::builtins::restore_queued_microtask_contexts();
        // A microtask's callback threw and unwound here. Read the
        // exception, clear it, and reject the `next` promise of the
        // microtask that was running. js_try_end is intentionally NOT
        // called yet — we want the trap to remain in scope for the
        // rest of the loop.
        let exc = crate::exception::js_get_exception();
        crate::exception::js_clear_exception();
        let cur = CURRENT_MICROTASK_PROMISE.with(|c| c.replace(std::ptr::null_mut()));
        CURRENT_MICROTASK_CALLBACK.with(|c| c.set(std::ptr::null()));
        CURRENT_MICROTASK_VALUE.with(|c| c.set(0.0));
        CURRENT_MICROTASK_NEXT.with(|c| c.set(std::ptr::null_mut()));
        if !cur.is_null() {
            unsafe {
                if !(*cur).next.is_null() {
                    js_promise_reject((*cur).next, exc);
                }
            }
            ran += 1;
        } else {
            let prev = INLINE_TRAP.with(|c| c.replace(InlineTrap::empty()));
            if !prev.trap_next.is_null() {
                js_promise_reject(prev.trap_next, exc);
                ran += 1;
            }
        }
    }

    // Drain queued microtasks (from queueMicrotask() calls) under the same
    // trap used for Promise callbacks so context is restored if a callback
    // throws through the runtime.
    crate::builtins::js_drain_queued_microtasks();

    // Cached profile flag — set once by mt_profile_register() above.
    // Reading the env var directly here was ~30 ns per microtask drain;
    // the atomic load is ~1 ns.
    let prof = mt_profile_enabled();
    loop {
        let t0 = if prof {
            Some(std::time::Instant::now())
        } else {
            None
        };
        let task = TASK_QUEUE.with(|q| q.borrow_mut().pop_front());
        if let Some(t) = t0 {
            MT_TIME_NS_QUEUE.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }

        match task {
            None => break,
            Some(Task::Promise(promise, value, is_fulfilled, task_context)) => {
                bump(&MT_RUN_COUNT);
                enter_microtask_context(&task_context);
                unsafe {
                    let callback = if is_fulfilled {
                        (*promise).on_fulfilled
                    } else {
                        (*promise).on_rejected
                    };

                    // No callback registered → propagate the value/reason
                    // to the next promise without invoking anything.
                    if callback.is_null() {
                        CURRENT_MICROTASK_PROMISE.with(|c| c.set(promise));
                        CURRENT_MICROTASK_VALUE.with(|c| c.set(value));
                        CURRENT_MICROTASK_NEXT.with(|c| c.set((*promise).next));
                        if !(*promise).next.is_null() {
                            if is_fulfilled {
                                js_promise_resolve((*promise).next, value);
                            } else {
                                js_promise_reject((*promise).next, value);
                            }
                        }
                        let promise =
                            CURRENT_MICROTASK_PROMISE.with(|c| c.replace(std::ptr::null_mut()));
                        CURRENT_MICROTASK_VALUE.with(|c| c.set(0.0));
                        CURRENT_MICROTASK_NEXT.with(|c| c.set(std::ptr::null_mut()));
                        clear_promise_context(promise);
                        restore_microtask_context();
                        ran += 1;
                        continue;
                    }

                    // Record the running promise so the trap (above)
                    // can reject its `next` if the callback throws.
                    //
                    // #1663: the callback can re-entrantly drain the
                    // microtask queue — a non-transformed async closure's
                    // `await` busy-waits on `js_promise_run_microtasks`, and
                    // each nested `Task::Promise` dispatch overwrites these
                    // same TLS cells (and clears them on exit). Reloading
                    // `promise` / `next` from the cells after the callback
                    // would then observe a stale or NULL pointer; the very
                    // next line dereferences `(*promise).async_id` (offset
                    // 0x30) and segfaults. Root our promise + next in a
                    // handle scope so we reload the GC-updated pointers from
                    // there, and save/restore the previous cell values so a
                    // nested drain leaves the enclosing arm — and its
                    // exception-trap routing — intact. This mirrors the
                    // INLINE_TRAP save/restore in the Inline/AsyncStep arms.
                    let scope = crate::gc::RuntimeHandleScope::new();
                    let promise_handle = scope.root_raw_mut_ptr(promise);
                    let next_handle = scope.root_raw_mut_ptr((*promise).next);
                    let prev_promise = CURRENT_MICROTASK_PROMISE.with(|c| c.get());
                    let prev_callback = CURRENT_MICROTASK_CALLBACK.with(|c| c.get());
                    let prev_value = CURRENT_MICROTASK_VALUE.with(|c| c.get());
                    let prev_next = CURRENT_MICROTASK_NEXT.with(|c| c.get());
                    let prev_promise_handle = scope.root_raw_mut_ptr(prev_promise);
                    let prev_next_handle = scope.root_raw_mut_ptr(prev_next);

                    CURRENT_MICROTASK_PROMISE.with(|c| c.set(promise));
                    CURRENT_MICROTASK_CALLBACK.with(|c| c.set(callback));
                    CURRENT_MICROTASK_VALUE.with(|c| c.set(value));
                    CURRENT_MICROTASK_NEXT.with(|c| c.set((*promise).next));

                    let t1 = if prof {
                        Some(std::time::Instant::now())
                    } else {
                        None
                    };
                    crate::async_hooks::before((*promise).async_id, (*promise).trigger_async_id);
                    let result = crate::closure::js_closure_call1(callback, value);
                    // Keep the callback result rooted across `after()` (which
                    // can run JS when async_hooks are active) via the value
                    // cell, then reload promise/next from our handles — never
                    // the TLS cells, which a re-entrant drain may have nulled.
                    CURRENT_MICROTASK_VALUE.with(|c| c.set(result));
                    let promise = promise_handle.get_raw_mut_ptr::<Promise>();
                    let next = next_handle.get_raw_mut_ptr::<Promise>();
                    crate::async_hooks::after((*promise).async_id);
                    if let Some(t) = t1 {
                        MT_TIME_NS_CALLBACK
                            .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    }

                    let t2 = if prof {
                        Some(std::time::Instant::now())
                    } else {
                        None
                    };
                    if !next.is_null() {
                        let result = CURRENT_MICROTASK_VALUE.with(|c| c.get());
                        propagate_callback_result(result, next);
                    }
                    clear_promise_context(promise);
                    if let Some(t) = t2 {
                        MT_TIME_NS_RESOLVE
                            .fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    }

                    // Restore the previous CURRENT_MICROTASK_* cells so an
                    // enclosing (re-entrant) dispatch resumes with its own
                    // promise/next/value for settlement and trap routing,
                    // instead of the NULLs this arm would otherwise leave.
                    CURRENT_MICROTASK_PROMISE
                        .with(|c| c.set(prev_promise_handle.get_raw_mut_ptr::<Promise>()));
                    CURRENT_MICROTASK_CALLBACK.with(|c| c.set(prev_callback));
                    CURRENT_MICROTASK_VALUE.with(|c| c.set(prev_value));
                    CURRENT_MICROTASK_NEXT
                        .with(|c| c.set(prev_next_handle.get_raw_mut_ptr::<Promise>()));
                }
                restore_microtask_context();
                ran += 1;
            }
            Some(Task::PromiseAll(state, value, is_fulfilled, task_context)) => {
                bump(&MT_RUN_COUNT);
                enter_microtask_context(&task_context);
                combinators::promise_all_settle(state, value, is_fulfilled);
                restore_microtask_context();
                ran += 1;
            }
            Some(Task::Inline(callback, value, next, is_fulfilled, task_context)) => {
                bump(&MT_RUN_COUNT);
                enter_microtask_context(&task_context);
                // Inline tasks are produced by `js_promise_resolved_then`
                // (the `Promise.resolve(<primitive>).then(cb_f, cb_e)`
                // fast path). We've already skipped allocating the
                // source promise — now dispatch directly: invoke the
                // stored callback, propagate the result to `next`.
                if callback.is_null() {
                    if !next.is_null() {
                        if is_fulfilled {
                            js_promise_resolve(next, value);
                        } else {
                            js_promise_reject(next, value);
                        }
                    }
                    restore_microtask_context();
                    ran += 1;
                    continue;
                }

                // For exception unwinding, mirror the Promise variant:
                // store a fake `cur` whose `.next` is what we want to
                // reject if the callback throws. Allocate a minimal
                // stub on the GC heap so the trap path still finds a
                // valid `*mut Promise`. This is rarely hit (only on
                // user-throw inside the inline callback) and we can
                // afford the alloc on the slow path.
                //
                // Issue #748: same save/restore reasoning as the
                // Task::AsyncStep arm below — preserve any outer
                // INLINE_TRAP (set by an enclosing `js_async_first_call`)
                // when the runner is invoked re-entrantly from inside
                // a non-transformed async closure's busy-wait.
                let prev_trap = INLINE_TRAP.with(|c| c.get());
                let trap_scope = crate::gc::RuntimeHandleScope::new();
                let prev_trap_next_handle = trap_scope.root_raw_mut_ptr(prev_trap.trap_next);
                let prev_trap_step_handle = trap_scope.root_raw_const_ptr(
                    prev_trap.current_step as *const crate::closure::ClosureHeader,
                );
                CURRENT_MICROTASK_CALLBACK.with(|c| c.set(callback));
                CURRENT_MICROTASK_VALUE.with(|c| c.set(value));
                CURRENT_MICROTASK_NEXT.with(|c| c.set(next));
                INLINE_TRAP.with(|c| {
                    c.set(InlineTrap {
                        trap_next: next,
                        current_step: 0,
                    })
                });

                let t1 = if prof {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let result = crate::closure::js_closure_call1(callback, value);
                CURRENT_MICROTASK_VALUE.with(|c| c.set(result));
                if let Some(t) = t1 {
                    MT_TIME_NS_CALLBACK.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }

                INLINE_TRAP.with(|c| {
                    c.set(InlineTrap {
                        trap_next: prev_trap_next_handle.get_raw_mut_ptr::<Promise>(),
                        current_step: prev_trap_step_handle
                            .get_raw_const_ptr::<crate::closure::ClosureHeader>()
                            as usize,
                    })
                });
                CURRENT_MICROTASK_CALLBACK.with(|c| c.set(std::ptr::null()));

                let t2 = if prof {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let next = CURRENT_MICROTASK_NEXT.with(|c| c.replace(std::ptr::null_mut()));
                if !next.is_null() {
                    let result = CURRENT_MICROTASK_VALUE.with(|c| c.replace(0.0));
                    propagate_callback_result(result, next);
                } else {
                    CURRENT_MICROTASK_VALUE.with(|c| c.set(0.0));
                }
                if let Some(t) = t2 {
                    MT_TIME_NS_RESOLVE.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }
                restore_microtask_context();
                ran += 1;
            }
            Some(Task::AsyncStep(step_closure, value, next, is_error, task_context)) => {
                bump(&MT_RUN_COUNT);
                enter_microtask_context(&task_context);
                // Direct dispatch of the async-step closure. Skips the
                // then_v_arrow / then_e_arrow wrapper that would
                // otherwise be invoked as the on_fulfilled / on_rejected
                // callback — the wrapper just calls
                // `__step(value, is_error)` which is exactly what we do
                // here with two fewer indirections (closure alloc +
                // closure call).
                if step_closure.is_null() {
                    if !next.is_null() {
                        if is_error {
                            js_promise_reject(next, value);
                        } else {
                            js_promise_resolve(next, value);
                        }
                    }
                    restore_microtask_context();
                    ran += 1;
                    continue;
                }
                CURRENT_MICROTASK_CALLBACK.with(|c| c.set(step_closure));
                CURRENT_MICROTASK_VALUE.with(|c| c.set(value));
                CURRENT_MICROTASK_NEXT.with(|c| c.set(next));
                // Issue #712 + #921 + #922 defensive guard. Track
                // consecutive is_error=true dispatches; reject the
                // chain if it crosses ASYNC_STEP_REENTRY_BOUND.
                //
                // Originally (#712) the guard required SAME `step_closure`
                // to count up — but the #921/#922 production loops
                // (gscmaster-api Fastify route handlers) alternate
                // between two async-step closures (route handler ↔
                // middleware ↔ inner await), each one rethrowing the
                // same TypeError. With the same-closure check, the
                // counter resets every other dispatch and the loop
                // never trips the guard — the user observed 5.7M
                // identical `value is not a function` lines before PM2
                // restarted the process.
                //
                // Drop the same-closure check: count ANY consecutive
                // run of `is_error=true` dispatches. A legitimate
                // throw-in-a-loop pattern interleaves `is_error=false`
                // steps (the loop's post-catch state) between throws,
                // so its consecutive count never grows beyond 1.
                if is_error {
                    let prev = ASYNC_STEP_GUARD.with(|c| c.get());
                    let new_count = prev.consecutive_error_count.saturating_add(1);
                    if new_count > ASYNC_STEP_REENTRY_BOUND {
                        ASYNC_STEP_GUARD.with(|c| {
                            c.set(AsyncStepGuard {
                                last_closure: 0,
                                consecutive_error_count: 0,
                            })
                        });
                        if !next.is_null() {
                            let msg = b"async step driver detected runaway re-entry (issue #712/#921/#922 guard); rejecting Promise to prevent unbounded loop. Common cause: throw across an await boundary inside try/catch; convert to a result-tag pattern.";
                            let msg_str =
                                crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
                            let err = crate::error::js_typeerror_new(msg_str);
                            let err_val = crate::value::js_nanbox_pointer(err as i64);
                            let next =
                                CURRENT_MICROTASK_NEXT.with(|c| c.replace(std::ptr::null_mut()));
                            js_promise_reject(next, err_val);
                        }
                        CURRENT_MICROTASK_CALLBACK.with(|c| c.set(std::ptr::null()));
                        CURRENT_MICROTASK_VALUE.with(|c| c.set(0.0));
                        CURRENT_MICROTASK_NEXT.with(|c| c.set(std::ptr::null_mut()));
                        restore_microtask_context();
                        ran += 1;
                        continue;
                    }
                    ASYNC_STEP_GUARD.with(|c| {
                        c.set(AsyncStepGuard {
                            last_closure: step_closure as usize,
                            consecutive_error_count: new_count,
                        })
                    });
                } else {
                    ASYNC_STEP_GUARD.with(|c| {
                        c.set(AsyncStepGuard {
                            last_closure: 0,
                            consecutive_error_count: 0,
                        })
                    });
                    // Issue #922: a non-error step dispatched, signalling
                    // forward progress through the user's async state
                    // machine. Reset the throw_not_callable counter so a
                    // legitimate later throw-in-a-loop doesn't trip the
                    // circuit breaker just because the program threw
                    // 100_000 cumulative times across the whole run.
                    crate::closure::reset_throw_not_callable_counter();
                }
                // Stash both trap_next + current_step in a single TLS
                // write so the hot path doesn't pay two `.with()` calls
                // per microtask. `current_step` gates the
                // `js_async_step_chain` / `js_async_step_done` reuse
                // path: nested async-fn calls pass a DIFFERENT step
                // closure → fail the gate → alloc their own next, so
                // their settlement can't collapse onto the parent's.
                //
                // Issue #748: save the previous INLINE_TRAP value and
                // restore it after step dispatch. The microtask runner
                // can be called RE-ENTRANTLY from inside an outer
                // async-step body — specifically when a non-transformed
                // async closure's `await` busy-waits on
                // `js_promise_run_microtasks()`. The outer body
                // (e.g. a top-level async function's state machine
                // closure) was entered via `js_async_first_call` which
                // set INLINE_TRAP to `{trap_next: null, current_step:
                // outer_step}`. Without save/restore, clearing to empty
                // after the inner Task::AsyncStep dispatch would leak
                // back to the outer body — `Expr::CurrentStepClosure`
                // (lowered to `js_get_current_step_closure`) returns
                // NULL after control returns from the busy-wait, and
                // the outer's `AsyncStepChain` queues a Task::AsyncStep
                // with step=NULL. That task hits the null-step short
                // circuit (line 1316) which only propagates the value
                // to `next` without ever calling the outer step body's
                // state-1 code — symptom: the outer body's post-await
                // statements never execute and the returned Promise
                // settles with the awaited value rather than the
                // explicit return expression.
                let prev_trap = INLINE_TRAP.with(|c| c.get());
                let trap_scope = crate::gc::RuntimeHandleScope::new();
                let prev_trap_next_handle = trap_scope.root_raw_mut_ptr(prev_trap.trap_next);
                let prev_trap_step_handle = trap_scope.root_raw_const_ptr(
                    prev_trap.current_step as *const crate::closure::ClosureHeader,
                );
                INLINE_TRAP.with(|c| {
                    c.set(InlineTrap {
                        trap_next: next,
                        current_step: step_closure as usize,
                    })
                });

                let t1 = if prof {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                let is_error_bits = if is_error {
                    f64::from_bits(0x7FFC_0000_0000_0004) // TAG_TRUE
                } else {
                    f64::from_bits(0x7FFC_0000_0000_0003) // TAG_FALSE
                };
                let result = call_async_step_direct(step_closure, value, is_error_bits);
                CURRENT_MICROTASK_VALUE.with(|c| c.set(result));
                if let Some(t) = t1 {
                    MT_TIME_NS_CALLBACK.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }

                INLINE_TRAP.with(|c| {
                    c.set(InlineTrap {
                        trap_next: prev_trap_next_handle.get_raw_mut_ptr::<Promise>(),
                        current_step: prev_trap_step_handle
                            .get_raw_const_ptr::<crate::closure::ClosureHeader>()
                            as usize,
                    })
                });
                CURRENT_MICROTASK_CALLBACK.with(|c| c.set(std::ptr::null()));

                let t2 = if prof {
                    Some(std::time::Instant::now())
                } else {
                    None
                };
                // Self-chain marker: when `js_async_step_chain` reused
                // our `next` Promise (the steady-state primitive-await
                // path), the result is the same Promise pointer. The
                // next iteration's `Task::AsyncStep` is already on the
                // queue carrying the same `next`; nothing to propagate
                // here.
                let next = CURRENT_MICROTASK_NEXT.with(|c| c.replace(std::ptr::null_mut()));
                if !next.is_null() {
                    let result = CURRENT_MICROTASK_VALUE.with(|c| c.replace(0.0));
                    let result_is_self_chain = if js_value_is_promise(result) != 0 {
                        crate::value::js_nanbox_get_pointer(result) as *mut Promise == next
                    } else {
                        false
                    };
                    if !result_is_self_chain {
                        propagate_callback_result(result, next);
                    }
                } else {
                    CURRENT_MICROTASK_VALUE.with(|c| c.set(0.0));
                }
                if let Some(t) = t2 {
                    MT_TIME_NS_RESOLVE.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                }
                restore_microtask_context();
                ran += 1;
            }
        }
    }

    crate::exception::js_try_end();

    ran
}

#[inline(always)]
fn call_async_step_direct(
    step_closure: *const crate::closure::ClosureHeader,
    value: f64,
    is_error_bits: f64,
) -> f64 {
    // Task::AsyncStep is only enqueued by Perry's async/await lowering.
    // Its closure is the compiler-generated two-argument state-machine
    // step (`__step(value, is_error)`), never a bound method/rest wrapper.
    // Dispatching through `js_closure_call2` would re-run the generic
    // closure strategy lookup for every await continuation; direct-call
    // the stored function pointer instead.
    unsafe {
        let func_ptr = (*step_closure).func_ptr;
        let func: extern "C" fn(*const crate::closure::ClosureHeader, f64, f64) -> f64 =
            std::mem::transmute(func_ptr);
        func(step_closure, value, is_error_bits)
    }
}

/// Common tail of a microtask: take the value the callback returned
/// and feed it into `next`. If the callback returned a Promise, the
/// chained promise must ADOPT that promise's eventual state per
/// ECMAScript spec (Issue #256) — store-and-resolve breaks deep
/// generator-state-machine chains.
#[inline]
fn propagate_callback_result(result: f64, next: *mut Promise) {
    if js_value_is_promise(result) != 0 {
        let inner = crate::value::js_nanbox_get_pointer(result) as *mut Promise;
        if !inner.is_null() && inner != next {
            js_promise_resolve_with_promise(next, inner);
        } else {
            js_promise_resolve(next, result);
        }
    } else {
        js_promise_resolve(next, result);
    }
}
