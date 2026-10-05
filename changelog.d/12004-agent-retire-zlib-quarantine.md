Fixed a leak of each `worker_threads` agent's zlib streams, listeners and queued
events: a retiring agent now releases them, and the next agent reuses the set.
The `PERRY_GC_PROTECT_OLD_SWEEP` instrument now forgets quarantined spans in
blocks that thread exit frees, so it no longer misreports a fault on reused
memory.
