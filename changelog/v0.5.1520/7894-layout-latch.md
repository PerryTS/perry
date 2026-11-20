Fixed the process-wide per-object GC layout gate so concurrent thread arm/disarm
transitions cannot publish a false zero, and workers that exit with live layout
records no longer leave the allocation fast path permanently armed.
