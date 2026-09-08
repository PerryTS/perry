**`for-in` no longer allocates a heap string and a hash entry for every own
name at every prototype level** (#9823).

`js_for_in_keys_value` kept a `HashSet<String>` of every own name — enumerable
or not — at every level of the prototype chain, so that a name owned closer to
the receiver hides the same name further along it (ECMA-262 14.7.5, 12.6.4-2).
It built that set unconditionally, which meant materialising a second key array
per level (all own names, on top of the enumerable ones) and turning every name
at every level into an owned `String` purely so it could be hashed.

That set can only filter a level at or below the first prototype, and a level
that contributes no enumerable keys of its own never consults it. It is now
built on demand — at the moment a prototype level actually has an enumerable
key to filter — from exactly the levels already walked, so the emitted key
sequence is unchanged.

On the compiled claude-code TUI, one 400-character reply: **159,947 `String`
allocations and 159,947 hash inserts become zero**, and the key arrays
materialised per call halve from 4.00 to 2.00. Across 17,281 `for-in` loops in
that reply, **no key was emitted from a prototype level at all**, so the set
that cost all of that filtered nothing. The strings totalled 1.91 MB, which is
why an allocation-byte ranking never surfaced this: the cost was 160,000
mallocs, memcpys, hashes and frees, not the bytes they held. The collection
schedule is unchanged (41 vs 43 copying minors, 46 vs 48 budgeted full-cycle
steps).

`PERRY_ENUM_DIAG=<path>` reports the counters above. `PERRY_FORIN_LAZY_SHADOW=0`
restores the eager set.
