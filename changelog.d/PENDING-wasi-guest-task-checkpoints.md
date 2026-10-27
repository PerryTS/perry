Add internal C task frames that retain traced ALS state and checkpoint only
eligible Promise, nextTick and deferred cleanup jobs. User ALS snapshots cannot
switch canonical task ownership. Cross-task Promise settlement marks the owner
ready, and uncaught task jobs preserve their original error inside the C frame
instead of exiting the host. Restore microtask runner counters and activation
boundaries before reporting a task error, so a sibling can continue processing
new jobs. Finished ownership leases allow later job adoption;
this is not cooperative cancellation.

Qualify task isolation, wake readiness, moving GC, teardown and C error boundaries
with Linux development and unprivileged optimized panic-abort runtime tests.
Keep public async guest generation guarded while canonical adapters are built.
