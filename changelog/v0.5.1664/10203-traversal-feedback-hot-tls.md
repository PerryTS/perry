### fix(json): route the traversal-feedback counters through the hot thread-local cache

`json/traversal_feedback.rs` declared its two per-thread counters with a raw `thread_local!`, which the thread-local policy ratchet (`scripts/check_thread_locals.py`) rejects; they now use `crate::perry_thread_local!` like every other runtime declaration, so the per-parse read lands in the hot TLS cache instead of a `_tlv_get_addr` call.
