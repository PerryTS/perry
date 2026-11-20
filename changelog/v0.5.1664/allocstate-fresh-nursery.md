Reduce per-allocation state lookup and redundant constructor barriers.

ELF executable code addresses the existing inline allocator cursor through native TLS, once per function activation, and keeps the existing hidden state parameter for recursive allocation bodies. Thread heap initialization primes that same cursor before main-thread or worker code runs. The generic initializer and other target/output paths remain available; no second heap, cursor, cache, or registry is introduced.

Complete constructor prologues consume a shared Fresh(parent) proof from emitted Eden allocation. Their pure stores finish before escape or any statepoint; nursery parents need no remembered-set entry, and a born-black parent's pending seed traces the finished payload. Outlined births and constructors with observable work retain barriers. The live birth-color branch and black-birth seeding protocol remain unchanged.

Coverage includes worker heap isolation, uninitialized-state negative control, wide pointer-bearing prologue IR, constructor escape/return/heritage cases, and allocation/root survival workloads.
