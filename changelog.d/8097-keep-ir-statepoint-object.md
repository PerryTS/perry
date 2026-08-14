`compiler-output-regression` is capturing again. All 11 of its workloads failed
with the same error — "PERRY_LLVM_KEEP_IR did not report a retained object
path" — so none of them reached the behaviour the gate exists to measure.

`PERRY_LLVM_KEEP_IR` promises that intermediates are retained *and* their
locations printed; the harness recovers them by parsing `kept object:` off the
compile log. The in-process backend has two success arms: the byte-returning
arm writes the object and announces it, while the statepoint arm — taken
whenever the plan asks for `-S`, so the ordinary path on every statepoint
target — assembles straight to `plan.obj_path` and keeps the scratch dir. That
arm retained the object correctly but never announced it, and the harness went
blind the moment statepoints became the default backend.

The statepoint arm now announces the kept object too; retention was already
correct, so this restores the reporting half of the contract rather than
changing what is kept.

`keep_ir_retains_the_whole_scratch_dir` only ever ran with `native_roots:
false`, so the arm serving every statepoint target had no coverage;
`keep_ir_retains_the_whole_scratch_dir_under_native_roots` now runs the same
contract through it.
