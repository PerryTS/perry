Runtime helpers are marked `nounwind` only when the linked runtime archive's
GC call-effect table proves they don't throw; the hand-written list is gone.
A runtime function that starts throwing now has its exception reach the JS
`catch` instead of being treated as non-unwinding. `Object.keys` results that
are forwarded or sparse resolve their length directly instead of going through
the generic (Proxy/getter) array-length path (#12743).
