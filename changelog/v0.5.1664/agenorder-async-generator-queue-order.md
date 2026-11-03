Async generators now complete the active request and resume queued requests in the
same microtask, matching Node and AsyncGeneratorYield / AsyncGeneratorCompleteStep.
The generator's promise settlement observer job and deferred queue drain are removed;
pending and immediate completion share one iterative drain. Return values and yielded
promises retain their required Await jobs. Producer completion uses the pending
promise's existing GC-scanned inactive result word, without growing Promise or adding
a registry. Node gap coverage includes overlapping next/return/throw, promise yields,
awaited finally, return during await, for-await, async-from-sync, rejection, and a long
completed queue.
Direct and thunk async-step runners now share rejection forwarding: an internal
catch wrapper completes the activation directly instead of being adopted through
promise reaction jobs. An additional gap test covers throw-after-await ordering.
Rejected return arguments now resume a suspended yield as a throw, preserving
the body's catch/finally. Abrupt PromiseResolve completion remains in the
iterative drain, including long completed tails with throwing constructors.

Rejected return arguments also close suspended-start generators through their
original throw transition before draining queued requests, without entering the
body. Remove the separate started flag. Keep the completion-presence check
inline so ordinary promise settlements avoid the rooted callback's prologue.
