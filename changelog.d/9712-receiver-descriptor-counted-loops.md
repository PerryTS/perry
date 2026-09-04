**Ordinary counted loops now consume the shared receiver descriptor model**
(#9254 phase 3). For a strict, call-free `i < array.length` numeric-array loop,
codegen validates the receiver and element layout once in the preheader, keeps
the boxed receiver precisely rooted, and carries a cached base handle through
the loop. Exact bounded `array[i]` reads select an invariant raw-load arm instead
of repeating the receiver tag, header, integrity, length, capacity, and layout
checks at every use; a failed one-time validation retains the existing guarded
fallback semantics.

Admission is intentionally conservative: the shared region analysis rejects
calls, allocations, coercions, suspensions, unwind edges, and every unmodelled
operation, while fired back-edge GC polls refresh both the rooted receiver and
its derived handle. Regression coverage pins the call-free fast block, the
allocating-region rejection, the guard-miss fallback, and a real rate-1 moving
collection with evacuated from-space protected.
