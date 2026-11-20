`is_registered_symbol` sits on property/method dispatch, so it is asked about
almost every pointer a program touches. `SYMBOL_EVER_REGISTERED` answered the
idle case in one atomic load, but stopped discriminating the moment a program
created its first symbol — and every program that touches a well-known symbol
creates one. After that each probe took the process-global registry mutex,
overwhelmingly to say "no".

The registry now carries the smallest and largest pointer ever registered, and
a pointer outside that range is rejected without the lock.
`PERRY_SYMBOL_RANGE_FILTER=0` restores the unconditional acquisition.

Unlike the buffer registries this pattern was lifted from, `SYMBOL_POINTERS` is
a GC ROOT registry: the collector re-keys an entry to the symbol's new address
every time it moves one. A range filter is sound only while every path that
puts a pointer in the set widens the bounds first, and there are three — the
registration, a per-slot forwarding rewrite, and the bulk rewrite that the
copying minor actually drives. Only the first widened, so a symbol evacuated
past the bounds its own allocation established became a live, registered symbol
that the probe reported as "not a symbol", losing `typeof`, symbol-keyed
property lookup and `Symbol.iterator` dispatch for it.

Widening and inserting are now a single operation (`insert_symbol_pointer_in_set`),
which is what makes the three sites uniform rather than three chances to forget.

`test_copying_minor_keeps_moved_symbol_visible_to_the_range_filter` covers it
through the public probe after a real copying minor. The existing sibling test
asserts membership with `test_symbol_pointer_root_contains`, which reads the set
directly and so could not see this — the entry IS in the set. The new test also
resets the range around a single registration, because a wide range left by
earlier tests in the same binary would admit the moved address by luck and make
the assertion vacuous.
