An explicit `--target` name that Perry does not know now fails with a diagnostic
that names the bad value and lists the accepted targets, instead of silently
compiling for the host. `--target linux-x86_64` had been recognized, but a
misspelling still emitted host objects — Mach-O arm64 on an Apple machine.
Backend-dispatched targets (`web`, `wasm`, the widget targets, `wearos-tile`)
and feature-gated aliases keep working; the accepted set is derived from the
codegen alias table and the backend dispatch, so it cannot drift.

The `perry-stdlib` test for a vm.Script released at thread exit no longer reads
the exited thread's heap after join, which could take the test binary down. The
verdict now comes from the thread-exit range hook the runtime already uses for
released thread-local state.

`scripts/check_file_size.sh` passes again: three allowlist entries recorded
code-line counts their files no longer had.
