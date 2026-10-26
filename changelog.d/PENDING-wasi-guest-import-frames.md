Retain guest import frames on the heap when a canonical C adapter explicitly
begins asynchronous work. The frame owns its Promise and captured ALS through
moving GC, and C receives ownership after the import callback returns, including
immediate completion. Synchronous imports still release immediately.

Add finite task resume callbacks and once-only import resolution/rejection that
requires the live originating task and uses normal Promise resolution. Check
reverse completion, thenable getters, pending Promise adoption, duplicate and
invalid completion, task expiry and teardown. Keep public async guest generation
guarded until the generated C scheduler and cancellation paths are qualified.
