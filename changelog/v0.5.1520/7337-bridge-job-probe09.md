`native-roots-aarch64` went red on `main`: #7330 made the explicit statepoint
bridge refuse a module it cannot root — every call inside a `try` is an `invoke`
since #7302, and the bridge cannot express a statepoint on one — but five steps
of that job still compiled `09_try_catch_roots` under the bridge, one of them by
hardcoded path. Nobody had seen it because CI has been stalled.

The bridge job now skips probe 09 in its four probe globs, and its report step
uses a non-try probe. `native-roots-rs4gc-aarch64` is unchanged and still covers
09, because RS4GC does handle invokes.

A skip that is not itself checked is just missing coverage, so the job also
asserts the refusal **happens**: compiling 09 under the bridge must fail, and
must fail with the #7327 diagnostic rather than for some unrelated reason. If the
bridge ever learns invokes, that step fails and says to re-enable 09 above.
