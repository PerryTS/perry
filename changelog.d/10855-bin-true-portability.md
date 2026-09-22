`test_gap_9592_child_timeout_threads.ts` spawned `/bin/true`, which does not
exist on macOS — it is `/usr/bin/true`. The fixture picks the path by platform
now (#10855).

Before, on macOS: the oracle itself died with
`Error: spawn /bin/true ENOENT`, node exited 1, and the harness scored perry
against node's stack trace as an `output mismatch`. Perry's side was equally
empty — the fixture guards its thread census with `process.platform === "linux"`
but left the spawn unguarded, so `timeoutThreadsReleased` starts `true` off
Linux and perry printed a constant. Neither side was measuring #9592's
thread-release behaviour.

After: node prints `timeout threads released: true` / `slow child killed on
time: true` and exits 0, and the harness reports PASS.

Blast radius checked rather than assumed: of the 15 fixtures hardcoding a
`/bin/` path, `/bin/bash`, `/bin/cat`, `/bin/echo`, `/bin/pwd`, `/bin/sh` and
`/bin/sleep` all exist on macOS. Only `/bin/true` does not, and only two
fixtures name it — the other, `test_gap_9537_child_process_null_bytes.ts`, uses
it purely as a string with an injected null byte to check node rejects the
argument before touching the filesystem, so it never spawns and is unaffected.

This is a fixture portability fix, not a perry change. The remaining half of
#10855 — that the non-Linux arm still asserts a constant and so cannot fail on
macOS — is left open there deliberately; making it measure something real is a
separate decision from making it runnable.
