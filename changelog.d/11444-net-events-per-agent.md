**fix(ext-net): each agent drains only its own socket events, so compiled pg and mysql2 inside `worker_threads` no longer hang or misdeliver events (#11340).** perry-ext-net kept one process-wide queue of pending socket events, and every push woke the primary thread. A socket opened on a worker therefore had its events drained by whichever agent's pump ran first.

When the primary took a worker's `'connect'`, `'data'` or `'close'`, it dispatched the event against listeners on the worker's heap. Depending on timing, the event was lost and the worker hung (pg's `connect()`), it ran on the wrong thread (a `TypeError` after the worker exited, as seen with mysql2), or the process crashed.

`pending_events()` now returns the calling agent's queue, keyed by the new `perry_ffi::agent_post::current_agent()`. Sockets, servers and in-flight closes record their owner agent, so an agent's keepalive counts only its own handles. A new gap test, `test_gap_11340_worker_net_events`, runs 25 pg-shaped client sockets from a worker: 2 of 20 runs passed on main, 20 of 20 with this fix. Verified against a live PostgreSQL 16.15 (pg 8.23.0) and MySQL 8.0.46 (mysql2 3.24.4).

A worker that hosts its own server still hangs intermittently. That is a separate defect, filed as #11434.
