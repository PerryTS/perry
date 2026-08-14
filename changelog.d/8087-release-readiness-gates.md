Restore the pre-release CI gates after several independent maintenance changes
had outpaced their checks. Test-only helpers and duplicated unwind context
declarations were still compiled in product targets, so the warning-denied jobs
failed even though normal builds succeeded; `eh.rs`, `eh_walker.rs`, and the GC
stack-map modules now share the real unwind ABI, while unused helpers in the GC
and CLI crates are limited to tests or removed. The structural audits had also
drifted: `property_set.rs` placed a valid pointer-free store marker outside the
inventory's bounded context, a test reused a class ID, and
`global_sink_isolation.py` treated immutable `RealmAtomic` handles as shared
state even though their mutable slots are `perry_thread_local!`. The audit now
resolves only the runtime's actual wrapper types, including qualified paths,
and rejects unrelated aliases.

GC allocation windows in `json_tape.rs` and `object/spill.rs` now reacquire raw
pointers through rooted handles, lowering the raw-handle debt baseline instead
of raising it. The two timer drain tests moved to
`timer/drain_expired_tests.rs`, returning `timer.rs` below the 2,000-line gate.
Finally, the Windows ARM64 workflow resolves the linked executable and invokes
it with PowerShell's call operator, so the smoke gate runs the artifact rather
than looking for its relative path in `PATH`.

Validation covered warning-denied runtime, product, and host-compatible
workspace checks; both CI clippy scopes; the pre-tag structural audit suite;
the raw-handle, store-site, class-ID, file-size, and global-sink self-tests and
real-tree audits; targeted moving-GC, unwind, timer, class, compile-cache, and
publish-config tests; workflow lint; the RustSec audit; and the Windows command
path through actionlint. The repository's gated release sweep and PR checks
provide the remaining platform-hosted coverage.
