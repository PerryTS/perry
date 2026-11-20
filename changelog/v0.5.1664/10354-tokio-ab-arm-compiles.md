**Repair the `tokio-wait-driver` A/B arm, which had stopped compiling.**

`tokio-wait-driver` is the baseline arm of the whole tokio-vs-turnloop
measurement: it compiles the legacy park instead of the per-agent turnloop loop.
It has not built since the P3 timers commit, which added
`pub(crate) use agent_loop::arm_timer as arm_agent_timer;` to `event_pump.rs`
**without** the `cfg` that every other item in that block carries. On the A/B
arm — and on `wasm32`, which shares the same gate — `mod agent_loop` is not
compiled at all, so the import resolves to nothing:

```
error[E0432]: unresolved import `agent_loop`
  --> crates/perry-runtime/src/event_pump.rs:37:16
```

Nothing caught it because no required gate builds that arm, and the last A/B run
predates P9. A measurement whose baseline does not compile is not a measurement;
this was found while checking an unrelated change against the arm.

The fix is the missing mirror: on the A/B and wasm arms `arm_agent_timer` is a
no-op, because there is no loop-owned timer handle to re-arm — the legacy park
recomputes its own timeout from the timer store on every pass.
