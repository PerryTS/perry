CI on `main`: the reds that are mechanical or stale are fixed — `cargo fmt`,
the two clippy errors (`STRS_K` comparison allowed while its cap is off;
`js_math_exp` constants written as `std::f64::consts`, same bits), a raw
`thread_local!` in `wpfacts_check/family.rs`, `dladdr` used without
`cfg(unix)` (Windows type-check), an empty `[[package]]` in a fixture
`Cargo.lock` (`tokio-ban`), stale entries the native-handle, GC-holder and
thread-exit gates report, two unregistered GC rooting witnesses, and two
AGENTS.md path spans the doc-claim audit rejected.
