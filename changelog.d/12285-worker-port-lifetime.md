Fix workers hanging after `parentPort.once("message")`, and expose `parentPort.close()`, `ref()`, `unref()` and `hasRef()` with MessagePort behavior. The parent port now uses the same listener lists, once consumption, asynchronous close delivery and reference state as MessageChannel ports, replacing its separate callback slots and GC scanner.

Named data exports now dispatch methods on their actual runtime value, rather than treating them as module APIs; the export's existing manifest kind determines this for every property export, without a parentPort method whitelist.

Workers re-evaluate event-loop liveness after every checkpoint and use the normal agent wait mechanism. Worker-owned timers, continuations and native work can finish after the module body returns; a consumed once listener or closed/unreferenced parent port no longer holds the worker open. Child Worker handles keep only their owning agent alive, replacing the process-wide live-worker mirror.

Event-handler lookup uses the existing shape-aware byte-key own-property getter, retaining accessors and prototype fallback while removing key-string allocations from port polling and dispatch.

Add Node parity witnesses for once, close, terminate, natural exit, deferred timer work, listener removal, reference transitions, EventTarget once listeners and nested workers, plus unit coverage for port state, listener rooting and agent ownership.
