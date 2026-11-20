Follow-up to #12514, which the merge train took while the `warnings` job
(`pr-gate`, `RUSTFLAGS=-D warnings`) was still running. Two of that job's
perry-runtime errors came from that change and are removed here; nothing else
about the events parity work changes.

`emit_process_warning`'s `js_object_get_field_by_name_f64` call was wrapped in
an `unsafe` block the function signature does not require, and
`process/warning_tests.rs` carried a `///` doc comment on a `thread_local!`
declaration, which `--all-targets` builds treat as an unused doc comment. Both
are compile-time only: the call is unchanged and the comment is now a plain
`//`. A dev-profile `cargo check -p perry-runtime --all-targets` now reports
nothing on any line either change touches.

The remaining failures in that `warnings` run are pre-existing and stay
pre-existing: 48 further `perry-runtime` errors (`arena/mod.rs`'s unused
`region::Kind as ArenaBlockKind` import, `proxy/json.rs`'s unused `handler`,
`object/census.rs`'s unused `st`, unnecessary `unsafe` blocks in
`object/shapes.rs` and `proxy/json.rs`) and 2 in `perry-codegen`. The other red
gates in the same run are likewise unrelated: `gc-call-effects` cannot compile
the tree (`object/builtin_proto.rs:503`, `expected u32, found usize`) and
`tls-budget`'s `check_thread_locals.py` lists pre-existing raw `thread_local!`
declarations and stale cold entries.
