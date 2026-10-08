Raw runtime handles are rooted across calls that can collect, and
`iterator_push_pending` re-reads its promise and iterator after it allocates.
The native-handle ledger counts the zlib stream's two handle tables again, and
the unrooted-local detector no longer treats `no_gc(` as a collection point.
