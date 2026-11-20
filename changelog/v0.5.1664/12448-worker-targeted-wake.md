Fix `worker_threads` programs stalling for whole seconds (worker_heavy: 0.2 s plus 0, 1, 2 or 3 s). Since #12285 workers park in the normal agent wait, which consumes the process-global notify flag. A worker could consume the notify that a sibling's `postMessage` (or exit) set for the parent. The parent then parked with the event already queued and slept until the 1 s idle cap. Worker → parent events now also send a targeted wake to the parent agent's own loop (`event_pump::notify_agent_loop`). That wake latches, so no other agent can consume it.

A targeted wake that reaches a claimed route before its loop is published is now kept on the route and delivered when the loop is published. Before this it was dropped, and the loop's first turn could block for the idle cap. Workers claim their route before publishing their agent id to the parent's sender. A Worker's creator claims its route when it constructs the Worker, so the wake can never find an unclaimed target.

The liveness check a worker runs after every callback is now O(1) (#12448). Each agent has counters for its queued worker events and its live, referenced Worker records. These replace scans of `PARENT_EVENTS` and every `WorkerRecord` ever created under two process-global locks. An agent's pump skips the shared event queue entirely when nothing is addressed to it, and drains its own events in place without rebuilding the queue. The MessagePort liveness probe no longer allocates.

Validation on qb6, with main and fix runs interleaved, 30 each:

- worker_heavy wall time: 0.21–0.26 s, mean 0.221 s. On main it was 0.21–4.23 s, with 24 of 30 runs stalled. The output hash is unchanged.
- User-space instructions: 1.937G, against 1.964G on main (−1.4%) and 1.926G on main with #12285 reverted (+0.56%).
- Worker loop turns: about 175 per worker, down from about 600.
- Negative control: dropping only the targeted wake brings the stall back (22 of 30 runs at 1.3–8.4 s).
- Gap tests: four worker gap tests stalled 1–5 s in every run on main (`worker_entry_from_url_helper`, `worker_namespace_ctor`, `worker_self_entry_path`, `worker_self_entry_state`), as did 4 of 20 runs of `worker_12285_terminate`. None stall now.

Unit tests cover a wake latched before the loop exists, and per-agent event counting and draining.
