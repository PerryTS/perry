Fixed `test_gap_turnloop_p9_worker_agent_net` failing intermittently (#11433). A `node:worker_threads` Worker doing `fetch` against a server owned by the main thread printed `immediate 000undefined`, threw `Invalid response handle`, or stalled at `WORKER STOPPED AFTER n/2`, because every JS agent runs the same process-wide pumps and three queues had no owner tag, so whichever agent pumped first drained all of them:

- perry-stdlib's promise-resolution queues (`common/async_bridge.rs`): a Worker's fetch promise could be settled on the main thread, and vice versa.
- The `worker_threads` Worker-to-parent event queue: a Worker could drain its own `postMessage` events, so the parent never received them.
- perry-ext-http's server pump: a Worker could run the main thread's `http.Server` handler, so the request was never answered.

Each entry now records its `AgentId`. Pumps, keep-alive checks and the async-bridge GC scanner act only on the calling agent's entries. Pool and fallback threads re-assert the submitting agent with `ResolutionOwnerScope`. `perry_runtime::agent::register_retire_hook` purges a retired agent's leftovers, and `perry_ffi::agent_post::current_agent()` exposes the agent id to ext crates. Under CPU load, the test went from 61/100 failures before the fix to 0/100 after.
