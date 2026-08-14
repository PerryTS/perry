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
Finally, the Windows ARM64 workflow initializes the ARM64 MSVC environment so
the linker can find the installed Windows SDK import libraries, then resolves
the linked executable and invokes it with PowerShell's call operator. The smoke
gate now reaches the final PE link and runs the artifact instead of relying on
an uninitialized SDK path or looking for the relative executable in `PATH`.

Validation covered warning-denied runtime, product, and host-compatible
workspace checks; both CI clippy scopes; the pre-tag structural audit suite;
the raw-handle, store-site, class-ID, file-size, and global-sink self-tests and
real-tree audits; targeted moving-GC, unwind, timer, class, compile-cache, and
publish-config tests; workflow lint; the RustSec audit; and the Windows command
path through actionlint. The repository's gated release sweep and PR checks
provide the remaining platform-hosted coverage.

Release-sweep tier 1 now mirrors that CI contract instead of running
`perry-runtime` inside a parallel workspace test: it excludes the runtime from
the normal workspace pass and invokes its release tests separately with
`RUST_TEST_THREADS=1`, preventing shared test-state races from masquerading as
release regressions.
