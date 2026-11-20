Parallel compiles no longer see each other's output kind. Two inputs a
compile depends on were process-wide: the `PERRY_CONSTFN_SHAPE` knob, read
from the environment on every module compile, and the "program is an
executable" flag that chose the local-exec TLS model in the object emitter.
Tests that flip either one raced with compiles running on other threads. The
codegen suite's `dispatch_tower_emission_is_run_to_run_deterministic` failed
about once in two thousand runs: one of its two compiles saw the knob set to
`0` and emitted every body info without `FN_PERMANENT_IMAGE` (bit 12 of the
flags word). Two ConstFn tests failed the same way, only in parallel runs.

The knob is now `CompileOptions::disable_constfn_shapes`, which the driver
reads once from the environment. The TLS model is written into the module's
own IR (`thread_local(localexec)`) by the compile that knows its output kind,
so the emitter, which runs on other threads, needs no outside input, and the
IR text states the model. The process-wide flag is gone.

Tests: `concurrent_compiles_of_different_output_kinds_are_deterministic`
compiles an executable, a dylib and an executable with ConstFn disabled on
three threads at once and checks each against its serial reference; module
tests pin the TLS rewrite.
