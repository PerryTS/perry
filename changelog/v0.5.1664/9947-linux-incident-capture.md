Added an external Linux process incident collector for server hangs and memory
growth, preserving per-thread CPU, memory snapshots, and optional perf evidence
without depending on the affected event loop. This instruments issue #9942;
it does not claim to fix the reported leak or hang.
