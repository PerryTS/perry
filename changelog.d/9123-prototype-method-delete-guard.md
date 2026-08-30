### fix(runtime): honor prototype method deletion in typed direct guards

Typed-feedback direct-method guards now consult the same per-name prototype
invalidation latch as inline shape guards. Deleting a declared prototype
method therefore falls back to ordinary dispatch and throws TypeError
instead of invoking the stale compiled method body. Fixes #9123.
