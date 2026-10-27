Allow supported WASI 0.3 HTTP handlers to serve concurrent requests on one
component Store. Each public Promise waits for its own transport flush and
validated task exit, preserving request ALS context through async imports and
moving GC. Private callback interposition preserves canonical resource identity
and keeps post-return traps observable. Completed task roots are released
individually while callback cleanup retains its own leases.

Add typed native HTTP submission to the persistent Rust/C driver and require
matching ABI 12 runtime and engine archives. Select unsupported nested/imported
handler layouts before initialization and retain their serialized behavior.
Keep stackful async and component threading disabled, and retain the no-Tokio
dependency policy. Add real concurrent plain/TLS exchanges, deferred C callbacks,
exit-validation failure, cancellation and protected moving-GC coverage to the
component checks.
