**Route four recent `thread_local!` declarations through the runtime's hot-TLS
cache.** `fs/deferred.rs`, `gc/idle_compact.rs`, `gc/idle_reclaim.rs` and
`gc/oldgen_defrag.rs` were added on 2026-09-03 using raw `thread_local!`, which
costs a `_tlv_get_addr` call per access instead of landing the address in this
thread's hot cache (#7469), and left `tls-budget` red on `main`. All four use
only the init forms `perry_thread_local!` accepts, so the conversion is
mechanical and behaviour-preserving.

Converting them made `gc_runtime_root_holders` see the declarations for the
first time — it enumerates `perry_thread_local!`, so a raw block escapes both
gates. `fs/deferred.rs`'s `PENDING_PATH_WRITES` is classified here;
`gc/census.rs` is deliberately left unconverted because its `PASS1_MARKED`
holds real GC header addresses that no existing verdict describes truthfully,
tracked in #9740.
