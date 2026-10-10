Precise-root admission is now O(1) and reads no malloc state. The arena region
descriptor decides ownership; outside every arena region the object header
decides. A root in another thread's live heap region (a Worker's, or a
zero-copy transferred cell) is still dropped without touching its header, but
every other root that is not this heap's object (a malformed header, an arena
header outside this heap, a stale or foreign malloc word) now fails loudly
under `PERRY_GC_VERIFY_MARK` instead of being skipped. The same verification
rejects a bare heap address stored in a JSValue global, temp, handle or
statepoint root. Copy-only pinning admits each root once.
