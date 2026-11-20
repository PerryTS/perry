fix(runtime): a worker's `async onmessage` handler resumes after its `await` and posts its reply (#10854).

After a worker's module body ran, the thread parked in a blocking receive and invoked the JS handler straight from there, then parked again. Nothing drained the microtask queue, so an `async` handler — which returns a pending promise — never got past its first `await`. The message was received and silently never answered: no rejection, no exception, no exit. Posting further messages did not drain the pending continuations either.

That is the ordinary shape for a request/response worker protocol, and it is what left the OpenCode TUI painting nothing. `Rpc.listen` does

```ts
onmessage = async (evt) => {
  const parsed = JSON.parse(evt.data)
  const result = await rpc[parsed.method](parsed.input)   // never resumed
  postMessage(JSON.stringify({ type: "rpc.result", result, id: parsed.id }))
}
```

so every request the TUI made was received and none answered. Its `Sync` provider gates on `status !== "loading"`, which only changes after a blocking `Promise.all` of six SDK calls over that RPC, so no provider below `Sync` ever mounted, nothing was inserted into the renderer root, and no frame was drawn — while `render()` still resolved, which is why it looked healthy.

Fix: the worker takes an event-loop turn after each delivered message and before parking again.

Two constraints the fix has to respect, both learned the hard way:

- It drains **microtasks/nextTicks only** (`js_promise_run_microtasks_await_loop`). It must not be the `AllowTimers` pump: `timer.rs` keeps `TIMER_QUEUE`, `CALLBACK_TIMERS` and `INTERVAL_TIMERS` in global mutexes rather than thread-locals, so that drain runs the MAIN thread's timer callbacks on the worker thread against the worker's globals. A later main-thread timer then died with `TypeError: value is not a function`, nondeterministically. The microtask/nextTick queues are `perry_thread_local!`, so draining those stays confined to the worker.
- The receive stays blocking when nothing is pending, so an idle worker costs exactly what it did before. When something is pending the wait is bounded, floored at 5ms — the timer queues are global, so a TUI's own 60fps render timers would otherwise wake every worker ~1000x/s for the life of the process.

Known remaining gap, tracked on the issue: `await` of a **timer** inside a worker handler still does not resume. The timer registers in the global queue and is run by whichever thread owns the event loop, resolving a promise that belongs to the worker's thread-local queue; that cross-thread ownership is a separate defect.

Second half: the worker also claims its own **agent id** (`enter_worker_agent`) at thread entry and retires it at exit. A `worker_threads` Worker gets its own arena and GC but never claimed an agent, so `current_agent()` fell back to `PRIMARY_AGENT` — `agent.rs` defines a thread with no agent of its own as a *pump acting for the primary heap*. The owner tag on `TIMER_QUEUE`/`CALLBACK_TIMERS`/`INTERVAL_TIMERS` therefore could not tell a worker's timers from the main thread's in either direction: the main thread fired timer closures living in the worker's arena, and an owner-filtered tick on the worker fired the main thread's. The `perry/thread` workers in `thread.rs` have always done this; the Web Worker path was simply missing it.

With the worker distinguishable, its pump can run its **own** timers through the owner-filtered tick, so `await` of a timer — or of anything a timer ultimately resolves — now resumes inside a worker too. That was the remaining gap: `await Promise.resolve()` and `await asyncFn()` were fixed by the microtask drain alone, but `await new Promise(r => setTimeout(r, 1))` still hung until the agent was claimed.
