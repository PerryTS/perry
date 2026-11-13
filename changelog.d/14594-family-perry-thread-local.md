The check-mode family claim's `SCRATCH` counter
(`perry-runtime/src/wpfacts_check/family.rs`) is declared with
`crate::perry_thread_local!` instead of a raw `thread_local!`, which the
thread-local policy check (`tls-budget`) refused.
