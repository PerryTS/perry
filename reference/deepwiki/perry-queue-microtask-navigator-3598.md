# DeepWiki Response

Repository: `PerryTS/perry`
Question: For PerryTS/perry, identify the current source files and test locations relevant to queueMicrotask, Promise microtask FIFO ordering, process.nextTick priority, and navigator/Navigator global shape for issue 3598. Focus on implementation hooks and deterministic parity fixtures.
Search: https://deepwiki.com/search/for-perrytsperry-identify-the_47f157a9-87c8-494c-ab32-6a63d9219f5f

**Question**: For PerryTS/perry, identify the current source files and test locations relevant to queueMicrotask, Promise microtask FIFO ordering, process.nextTick priority, and navigator/Navigator global shape for issue 3598. Focus on implementation hooks and deterministic parity fixtures.

The user is asking to identify source files and test locations related to `queueMicrotask`, Promise microtask FIFO ordering, `process.nextTick` priority, and `navigator/Navigator` global shape within the `PerryTS/perry` repository, specifically for issue 3598, focusing on implementation hooks and deterministic parity fixtures.

## `queueMicrotask` and `process.nextTick` Implementation and FIFO Ordering [1](#ref-q1-1)
The core implementation for `queueMicrotask` and `process.nextTick` is found in `crates/perry-runtime/src/builtins/globals.rs` [1](#ref-q1-1) . The `js_queue_microtask` function schedules a closure on the microtask queue [2](#ref-q1-2) . Similarly, `js_queue_next_tick` and `js_queue_next_tick_args` handle `process.nextTick` calls, with `js_queue_next_tick_args` supporting trailing arguments [3](#ref-q1-3) . Both ultimately call `queue_microtask_with_type` to add the task to the `QUEUED_MICROTASKS` thread-local queue [4](#ref-q1-4) .

The `js_drain_queued_microtasks` function, called by `js_promise_run_microtasks`, processes these queued microtasks [5](#ref-q1-5) . It iterates through the `QUEUED_MICROTASKS` queue, removing tasks from the front (`queue.remove(0)`) to ensure FIFO ordering [6](#ref-q1-6) . This function also handles `AsyncContextSnapshot` for context preservation and `async_hooks` for instrumentation [7](#ref-q1-7) .

The `js_promise_run_microtasks` function in `crates/perry-runtime/src/promise/microtasks.rs` is responsible for draining the microtask queue [8](#ref-q1-8) . It first drains other queues like `gc_destroy_queue`, `scheduled_resolves`, `diagnostics_channel_process_pending`, and `js_thread_process_pending`, and then calls `js_drain_queued_microtasks` to process `queueMicrotask` and `process.nextTick` tasks [9](#ref-q1-9)  [10](#ref-q1-10) . This establishes the priority of `process.nextTick` and `queueMicrotask` tasks over timers, matching Node's turn ordering [11](#ref-q1-11) .

The lowering of `queueMicrotask` and `process.nextTick` calls in the compiler is handled in `crates/perry-codegen/src/expr/instance_misc1.rs` [12](#ref-q1-12) . `Expr::QueueMicrotask` lowers to a call to `js_queue_microtask` [13](#ref-q1-13) , and `Expr::ProcessNextTick` lowers to either `js_queue_next_tick` or `js_queue_next_tick_args` depending on the presence of additional arguments [14](#ref-q1-14) . These runtime functions are declared in `crates/perry-codegen/src/runtime_decls/strings.rs` [15](#ref-q1-15) .

## Promise Microtask FIFO Ordering [16](#ref-q1-16)
The `TASK_QUEUE` in `crates/perry-runtime/src/promise/mod.rs` is a `VecDeque`, which ensures FIFO ordering for Promise microtasks [17](#ref-q1-17) . This queue is drained by `js_promise_run_microtasks` [18](#ref-q1-18) . The `Task` enum defines different types of microtasks, including `Promise`, `PromiseAll`, `Inline`, and `AsyncStep` [19](#ref-q1-19) .

## `navigator/Navigator` Global Shape for Issue 3598
The provided context does not contain specific information regarding the `navigator/Navigator` global shape or issue 3598.

## Implementation Hooks and Deterministic Parity Fixtures
The microtask draining process includes several hooks:
*   `drain_gc_destroy_queue()` [20](#ref-q1-20)
*   `process_scheduled_resolves()` [21](#ref-q1-21)
*   `diagnostics_channel_process_pending()` [22](#ref-q1-22)
*   `js_thread_process_pending()` [23](#ref-q1-23)
These are called before processing the main `TASK_QUEUE` [24](#ref-q1-24) .

The `js_promise_run_microtasks` function also incorporates an exception trap using `setjmp` to handle errors within microtask callbacks, ensuring that a `throw` from a callback rejects the chained `next` promise without exiting the microtask loop [25](#ref-q1-25) .

For `queueMicrotask` and `process.nextTick` callbacks, `async_hooks::init_resource`, `async_hooks::before`, and `async_hooks::after` are used to manage asynchronous context and provide instrumentation [26](#ref-q1-26)  [27](#ref-q1-27)  [28](#ref-q1-28) .

The event loop in `crates/perry-codegen/src/codegen/entry.rs` demonstrates the overall orchestration, where `js_promise_run_microtasks` and timer ticks are repeatedly called to process events [29](#ref-q1-29) . This ensures deterministic execution order.

## Notes
The prompt mentions "issue 3598" and "navigator/Navigator global shape". While the provided context extensively covers microtask handling, `queueMicrotask`, `process.nextTick`, and Promise ordering, there is no direct mention of "issue 3598" or the "navigator/Navigator" global object. It's possible that these aspects are handled in other parts of the codebase not included in the provided snippets, or that "issue 3598" refers to a specific bug related to the microtask queue behavior that is implicitly addressed by the described implementation.

Wiki pages you might want to explore:
- [Parsing & HIR Lowering (PerryTS/perry)](/wiki/PerryTS/perry#2.1)
- [Closures, Promises & Async Runtime (PerryTS/perry)](/wiki/PerryTS/perry#3.4)

View this search on DeepWiki: https://deepwiki.com/search/for-perrytsperry-identify-the_47f157a9-87c8-494c-ab32-6a63d9219f5f

## References

<a id="ref-q1-1"></a>
### [1] `crates/perry-runtime/src/builtins/globals.rs:376-395`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L376-L395)

```rust
// ============================================================
// queueMicrotask
// ============================================================

/// queueMicrotask(callback) — schedule a closure on the microtask queue.
/// The closure runs during the next `js_promise_run_microtasks()` drain,
/// AFTER the current synchronous code completes. Previously this called
/// the closure immediately, which broke the JS spec ordering:
///   queueMicrotask(() => log("micro"));
///   log("sync");
/// should print "sync" then "micro", not "micro" then "sync".
#[no_mangle]
pub extern "C" fn js_queue_microtask(callback: i64) {
    queue_microtask_with_type(callback, "Microtask", Vec::new());
}

#[no_mangle]
pub extern "C" fn js_queue_next_tick(callback: i64) {
    queue_microtask_with_type(callback, "TickObject", Vec::new());
}
```

<a id="ref-q1-2"></a>
### [2] `crates/perry-runtime/src/builtins/globals.rs:387-389`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L387-L389)

```rust
#[no_mangle]
pub extern "C" fn js_queue_microtask(callback: i64) {
    queue_microtask_with_type(callback, "Microtask", Vec::new());
```

<a id="ref-q1-3"></a>
### [3] `crates/perry-runtime/src/builtins/globals.rs:393-413`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L393-L413)

```rust
pub extern "C" fn js_queue_next_tick(callback: i64) {
    queue_microtask_with_type(callback, "TickObject", Vec::new());
}

/// process.nextTick(cb, ...args) — forwards trailing args to `cb` when the
/// tick fires (#1351). `args_ptr`/`n_args` describe a NaN-boxed-f64 buffer
/// allocated on the caller's stack; we copy the slice eagerly because the
/// drain runs after the caller returns.
///
/// # Safety
/// `args_ptr` must point to `n_args` valid `f64` values, or be null if
/// `n_args == 0`.
#[no_mangle]
pub unsafe extern "C" fn js_queue_next_tick_args(callback: i64, args_ptr: *const f64, n_args: i32) {
    let args: Vec<f64> = if args_ptr.is_null() || n_args <= 0 {
        Vec::new()
    } else {
        std::slice::from_raw_parts(args_ptr, n_args as usize).to_vec()
    };
    queue_microtask_with_type(callback, "TickObject", args);
}
```

<a id="ref-q1-4"></a>
### [4] `crates/perry-runtime/src/builtins/globals.rs:415-431`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L415-L431)

```rust
fn queue_microtask_with_type(callback: i64, type_name: &str, args: Vec<f64>) {
    let context = crate::async_context::capture_context();
    let ids = crate::async_hooks::init_resource(
        type_name,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
    QUEUED_MICROTASKS.with(|q| {
        q.borrow_mut().push(QueuedMicrotask {
            callback,
            context,
            async_id: ids.async_id,
            trigger_async_id: ids.trigger_async_id,
            args,
        });
    });
}
```

<a id="ref-q1-5"></a>
### [5] `crates/perry-runtime/src/builtins/globals.rs:455-457`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L455-L457)

```rust
/// Drain queued microtasks. Called by `js_promise_run_microtasks`.
#[no_mangle]
pub extern "C" fn js_drain_queued_microtasks() {
```

<a id="ref-q1-6"></a>
### [6] `crates/perry-runtime/src/builtins/globals.rs:463-468`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L463-L468)

```rust
        let task = QUEUED_MICROTASKS.with(|q| {
            let mut queue = q.borrow_mut();
            if queue.is_empty() {
                None
            } else {
                Some(queue.remove(0))
```

<a id="ref-q1-7"></a>
### [7] `crates/perry-runtime/src/builtins/globals.rs:473-487`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L473-L487)

```rust
                callback: cb,
                context,
                async_id,
                trigger_async_id,
                args,
            }) => {
                let scope = crate::gc::RuntimeHandleScope::new();
                let callback_handle =
                    scope.root_raw_const_ptr(cb as *const crate::closure::ClosureHeader);
                let arg_handles = scope.root_nanbox_f64_slice(&args);
                let previous = crate::async_context::enter_context(&context);
                QUEUED_MICROTASK_PREV_CONTEXTS.with(|stack| {
                    stack.borrow_mut().push(previous);
                });
                crate::async_hooks::before(async_id, trigger_async_id);
```

<a id="ref-q1-8"></a>
### [8] `crates/perry-runtime/src/promise/microtasks.rs:26-27`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L26-L27)

```rust
#[no_mangle]
pub extern "C" fn js_promise_run_microtasks() -> i32 {
```

<a id="ref-q1-9"></a>
### [9] `crates/perry-runtime/src/promise/microtasks.rs:31-41`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L31-L41)

```rust
    ran += crate::async_hooks::drain_gc_destroy_queue();

    // Process any scheduled resolutions (simulates async completions)
    ran += super::combinators::process_scheduled_resolves();

    // Process diagnostics_channel publishes queued by perry/thread workers.
    ran += crate::node_submodules::diagnostics_channel_process_pending();

    // Process pending thread results (from perry/thread spawn)
    ran += crate::thread::js_thread_process_pending();
```

<a id="ref-q1-10"></a>
### [10] `crates/perry-runtime/src/promise/microtasks.rs:126`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L126)

```rust
    crate::builtins::js_drain_queued_microtasks();
```

<a id="ref-q1-11"></a>
### [11] `crates/perry-runtime/src/promise/microtasks.rs:586-588`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L586-L588)

```rust
    // Timers run after already-queued promise/queueMicrotask jobs, matching
    // Node's turn ordering (`Promise.resolve().then(...)` before
    // `setTimeout(..., 0)`). Timer callbacks may enqueue more microtasks;
```

<a id="ref-q1-12"></a>
### [12] `crates/perry-codegen/src/expr/instance_misc1.rs:454-496`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/expr/instance_misc1.rs#L454-L496)

```rust
        // -------- queueMicrotask(fn) stub --------
        Expr::QueueMicrotask(cb) => {
            let cb_box = lower_expr(ctx, cb)?;
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void("js_queue_microtask", &[(I64, &cb_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }

        // -------- process.nextTick(fn, ...args) --------
        // Trailing args are forwarded to the callback when the tick fires
        // (#1351). Pack them into a stack buffer of doubles and hand off to
        // the varargs runtime entry; the no-args form goes through the
        // simpler `js_queue_next_tick` to avoid the alloca cost.
        Expr::ProcessNextTick { callback, args } => {
            let cb_box = lower_expr(ctx, callback)?;
            if args.is_empty() {
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                blk.call_void("js_queue_next_tick", &[(I64, &cb_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let n = args.len();
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void(
                "js_queue_next_tick_args",
                &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
            );
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
        }
```

<a id="ref-q1-13"></a>
### [13] `crates/perry-codegen/src/expr/instance_misc1.rs:455-460`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/expr/instance_misc1.rs#L455-L460)

```rust
        Expr::QueueMicrotask(cb) => {
            let cb_box = lower_expr(ctx, cb)?;
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void("js_queue_microtask", &[(I64, &cb_handle)]);
            Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)))
```

<a id="ref-q1-14"></a>
### [14] `crates/perry-codegen/src/expr/instance_misc1.rs:468-494`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/expr/instance_misc1.rs#L468-L494)

```rust
        Expr::ProcessNextTick { callback, args } => {
            let cb_box = lower_expr(ctx, callback)?;
            if args.is_empty() {
                let blk = ctx.block();
                let cb_handle = unbox_to_i64(blk, &cb_box);
                blk.call_void("js_queue_next_tick", &[(I64, &cb_handle)]);
                return Ok(double_literal(f64::from_bits(crate::nanbox::TAG_UNDEFINED)));
            }
            let n = args.len();
            let buf = ctx.func.alloca_entry_array(DOUBLE, n);
            for (i, a) in args.iter().enumerate() {
                let v = lower_expr(ctx, a)?;
                let blk = ctx.block();
                let slot = blk.gep(DOUBLE, &buf, &[(I64, &format!("{}", i))]);
                blk.store(DOUBLE, &v, &slot);
            }
            let ptr_reg = ctx.block().next_reg();
            ctx.block().emit_raw(format!(
                "{} = getelementptr [{} x double], ptr {}, i64 0, i64 0",
                ptr_reg, n, buf
            ));
            let blk = ctx.block();
            let cb_handle = unbox_to_i64(blk, &cb_box);
            blk.call_void(
                "js_queue_next_tick_args",
                &[(I64, &cb_handle), (PTR, &ptr_reg), (I32, &n.to_string())],
            );
```

<a id="ref-q1-15"></a>
### [15] `crates/perry-codegen/src/runtime_decls/strings.rs:1002-1011`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/runtime_decls/strings.rs#L1002-L1011)

```rust
    // Microtask queue (queueMicrotask / process.nextTick).
    module.declare_function("js_queue_microtask", VOID, &[I64]);
    module.declare_function("js_queue_next_tick", VOID, &[I64]);
    // #1351: process.nextTick(cb, ...args) — trailing args packed into a
    // stack buffer of doubles, forwarded when the tick fires.
    module.declare_function(
        "js_queue_next_tick_args",
        VOID,
        &[I64, crate::types::PTR, I32],
    );
```

<a id="ref-q1-16"></a>
### [16] `crates/perry-runtime/src/promise/mod.rs:377-382`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/mod.rs#L377-L382)

```rust
// Global task queue for pending promise callbacks. Must be FIFO per
// ECMAScript microtask semantics: `Promise.resolve(1).then(...)` and
// `Promise.resolve(2).then(...)` registered in source order must run
// their continuations in source order (1 first, then 2). Using a
// `Vec` with `.pop()` produces LIFO ordering, breaking every test
// that prints inside multiple parallel promise chains.
```

<a id="ref-q1-17"></a>
### [17] `crates/perry-runtime/src/promise/mod.rs:383-385`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/mod.rs#L383-L385)

```rust
thread_local! {
    pub(crate) static TASK_QUEUE: RefCell<std::collections::VecDeque<Task>>
        = const { RefCell::new(std::collections::VecDeque::new()) };
```

<a id="ref-q1-18"></a>
### [18] `crates/perry-runtime/src/promise/microtasks.rs:138-140`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L138-L140)

```rust
        let task = TASK_QUEUE.with(|q| q.borrow_mut().pop_front());
        if let Some(t) = t0 {
            MT_TIME_NS_QUEUE.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
```

<a id="ref-q1-19"></a>
### [19] `crates/perry-runtime/src/promise/mod.rs:359-374`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/mod.rs#L359-L374)

```rust
pub(crate) enum Task {
    Promise(*mut Promise, f64, bool, AsyncContextSnapshot),
    PromiseAll(
        combinators::PromiseAllState,
        f64,
        bool,
        AsyncContextSnapshot,
    ),
    Inline(ClosurePtr, f64, *mut Promise, bool, AsyncContextSnapshot),
    /// Direct dispatch to a 2-arg async-step closure. Equivalent to
    /// `Inline(then_v_arrow, value, next, true)` where `then_v_arrow`
    /// is a wrapper that calls `step(value, is_error)` — but skips the
    /// then_v_arrow alloc + dispatch by carrying `step_closure` and
    /// the `is_error` flag directly. Saves one closure allocation
    /// per await on the steady-state primitive-await path.
    AsyncStep(ClosurePtr, f64, *mut Promise, bool, AsyncContextSnapshot),
```

<a id="ref-q1-20"></a>
### [20] `crates/perry-runtime/src/promise/microtasks.rs:31`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L31)

```rust
    ran += crate::async_hooks::drain_gc_destroy_queue();
```

<a id="ref-q1-21"></a>
### [21] `crates/perry-runtime/src/promise/microtasks.rs:34`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L34)

```rust
    ran += super::combinators::process_scheduled_resolves();
```

<a id="ref-q1-22"></a>
### [22] `crates/perry-runtime/src/promise/microtasks.rs:37`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L37)

```rust
    ran += crate::node_submodules::diagnostics_channel_process_pending();
```

<a id="ref-q1-23"></a>
### [23] `crates/perry-runtime/src/promise/microtasks.rs:40`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L40)

```rust
    ran += crate::thread::js_thread_process_pending();
```

<a id="ref-q1-24"></a>
### [24] `crates/perry-runtime/src/promise/microtasks.rs:42`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L42)

```rust
    // Then process the task queue.
```

<a id="ref-q1-25"></a>
### [25] `crates/perry-runtime/src/promise/microtasks.rs:44-56`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/promise/microtasks.rs#L44-L56)

```rust
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
```

<a id="ref-q1-26"></a>
### [26] `crates/perry-runtime/src/builtins/globals.rs:417-421`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L417-L421)

```rust
    let ids = crate::async_hooks::init_resource(
        type_name,
        f64::from_bits(crate::value::TAG_UNDEFINED),
        false,
    );
```

<a id="ref-q1-27"></a>
### [27] `crates/perry-runtime/src/builtins/globals.rs:487`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L487)

```rust
                crate::async_hooks::before(async_id, trigger_async_id);
```

<a id="ref-q1-28"></a>
### [28] `crates/perry-runtime/src/builtins/globals.rs:527`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-runtime/src/builtins/globals.rs#L527)

```rust
                crate::async_hooks::after(async_id);
```

<a id="ref-q1-29"></a>
### [29] `crates/perry-codegen/src/codegen/entry.rs:469-472`
Source: [PerryTS/perry @ c720d2a4](https://github.com/PerryTS/perry/blob/c720d2a4/crates/perry-codegen/src/codegen/entry.rs#L469-L472)

```rust
                let _ = ctx.block().call(I32, "js_promise_run_microtasks", &[]);
                let _ = ctx.block().call(I32, "js_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_callback_timer_tick", &[]);
                let _ = ctx.block().call(I32, "js_interval_timer_tick", &[]);
```
