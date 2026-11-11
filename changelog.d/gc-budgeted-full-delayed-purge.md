- **GC: an ordinary full collection no longer forces the native allocator to decommit.** The reclaim tail of every
  full collection called `mi_collect(true)`, which decommits every free mimalloc page at once; on a program with
  frequent fulls the next allocation window faulted those pages straight back in. A full started by the heap's own
  growth now calls `mi_collect(false)`, and freed pages return to the OS through mimalloc's own purge delay. A full
  that owes the memory back now (`gc()`, an emergency full, critical memory pressure, idle reclaim) still forces
  the purge, as do the idle hooks, so an idle process still gets the pages back once the idle reclaimer runs.
