### fix(perf_hooks): complete `performance.timerify()` parity

Timerified async functions now emit their performance entry and optional
histogram sample when the returned promise settles, rather than when it is
created. The wrapper also preserves constructor behavior, including class-call
errors and `new.target` forwarding, while returning the original call result.

Function entries now expose call arguments through both `entry.detail` and
indexed entry properties. Timerify wrappers also carry Node-compatible
non-writable, enumerable, non-configurable `name` and `length` descriptors.
Fixes #8234.
