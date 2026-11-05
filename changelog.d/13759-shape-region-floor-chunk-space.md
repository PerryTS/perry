The shape Region's smallest reservation is the static prefix plus the chunk
space again (256 MiB of address space), not 516 MiB. Only layout-word rows
index the Region; the WeakMap/WeakSet birth layouts are seeded at the rows of
their declared words, which lie in the static prefix, and a compile-time check
now keeps them there. A process under a 2 GiB address-space limit (the
protected-poll reclaim gap test) starts again.
