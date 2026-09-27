Stop inferring the input element type for Array.map and Array.flatMap results.
Their callbacks can return entirely different values; inheriting string[] from
the receiver sent class methods such as node-cron's task.match() through the
String.match fast path and returned null instead of calling the task method.

Use an unknown element type in the early callee-only inference table and retain
the existing callback-aware HIR refinement. Add lowering regressions and native
parity coverage for mapped class instances, flatMap, and ordinary string and
numeric results.
