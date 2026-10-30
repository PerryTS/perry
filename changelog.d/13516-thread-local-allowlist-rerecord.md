Re-record the raw thread-local allowlist so the policy check passes on main
again: two per-GC-cycle statistics counters (`gc/leaf_service.rs`,
`gc/progress.rs`) are recorded as cold, and two files whose raw declarations
were converted (`gc/telemetry.rs`, `node_submodules/mod.rs`) drop to their
current counts. No runtime change.
