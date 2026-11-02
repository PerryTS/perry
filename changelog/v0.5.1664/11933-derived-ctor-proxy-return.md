A Proxy returned from a derived constructor after `super()` no longer faults the
runtime. The constructor-return check probed the Proxy id as a heap address
(reading the GC header just below it) before it asked whether the value was a
Proxy, so the eighth Proxy of a process could crash with SIGSEGV. The Proxy
check now runs first.
