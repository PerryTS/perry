The Linux `cargo-test` gate no longer aborts with a stack overflow while
collecting a statically reachable JavaScript package (#9196).

The module graph was finite: the debugger instead found
`lower_body_stmt`'s roughly 144 KiB unoptimized frame repeated across ordinary
nested blocks in Perry's synthesized CommonJS wrapper. A handful of those
bounded frames exhausted libtest's default 2 MiB worker stack before the
expression lowerer's existing stack guard could run.

Statement lowering now checks the remaining stack before entering that large
frame and grows onto a bounded segment near the red zone, matching expression
and type lowering. The heavy implementation stays behind a non-inlined call
boundary so debug builds cannot move its frame ahead of the guard. The complete
1,060-test `perry` binary passes on Linux without a global stack-size override.
