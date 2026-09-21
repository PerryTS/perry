Fixed `for await (const chunk of process.stdin)` on a pipe stalling forever
part-way through the input (#10895).

The async iterator pauses its source after every delivered chunk and resumes
it on the next pull. On `process.stdin`, `pause()` latches `STDIN_DETACHED` —
the fd-0 reader thread exits when it sees the latch at the top of its loop —
and `resume()` clears the latch and respawns the reader unless
`STDIN_READER_STARTED` says one is still running. The reader's stop decision
and its `STARTED` reset were two separate steps, so a `resume()` that landed
between them found `STARTED` still true, spawned nothing, and the old reader
then left: fd 0 had no reader while every liveness view still reported an
open, flowing stdin, and the process idled forever with input unread. One roll
per delivered chunk: near-certain at 4 MiB through a 16 KiB macOS pipe, 1 in
350 at 16 MiB on Linux. Introduced by the unified fd-0 reader (2026-09-04);
the published 0.5.1520 predates it.

The reader's check-and-clear and the restart CAS are now atomic with respect
to each other under one lifecycle lock (never held across `read()`); the
detach exit releases the reader slot itself and disarms the drop guard.

Tests: `crates/perry/tests/issue_10895_stdin_pipe_stall.rs` (8 MiB in 256-byte
writes, 12 rounds, red on unpatched main) and `reader_lifecycle_tests` in
`os_process_streams.rs`, which replays the interleaving deterministically.
