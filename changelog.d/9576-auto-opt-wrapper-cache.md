**fix(compile): isolate auto-optimized shared-tokio wrapper graphs (#9470, #9094)**

Auto-optimized builds now include the canonical shared-tokio wrapper package
set in their `target/perry-auto-<hash>` cache identity. Previously, programs
with the same stripped stdlib features but different ext wrappers shared one
target directory. A concurrent wrapper-free build could replace the stdlib
after an axios, mysql2, or HTTP wrapper build released its lock but before that
compiler linked, producing mismatched tokio/futures-channel archives. Wrapper
aliases such as `mysql2` / `mysql2/promise` and `http` / `https` are also
deduplicated before Cargo and the linker see them, and compile-smoke is strict
again now that its two temporary #9470 exemptions pass.
