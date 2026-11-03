A synchronous full mark-sweep now reclaims an arena block without walking its
objects when the cycle's exact pointer census shows the trace reached nothing
in it and nothing in it owes per-object sweep work (no finalizer, no pinned,
forwarded or pre-marked header, no raw-f64 array layout bits, and no
address-keyed side-table entry the dead-owner prune does not already drop).
The census seals its address-ordered runs at block boundaries, so a successful
membership query names the reached block at no extra lookup; the block cleanup
still resets or releases the block and drops its old-page index, and freed
bytes come from the census's per-block sums. The require-marked old-to-young
remembered-set rebuild skips the same unreached blocks. Budgeted fulls and
minors keep the per-object walk. `PERRY_GC_DIAG=1` prints
`block_skip_reclaimed_blocks/_objects/_bytes` on each `[gc] blocks:` line.

Measured on a loop that parses `records_array_8m.json`, keeps one tree, and
calls `gc()` each iteration: sweep 5.4 → 2.6 ms and 12–15 → 3.7–4.0 ms on the
fulls that follow a minor, atomic finalize 3.3 → 2.2 ms, census 2.7 → 3.3 ms;
the six fulls total 159 ms against 203 ms on the base. gc-ratchet gated
counters are unchanged on all 14 probes (block skip is live on 08, 09, 11 and
14), and the 22-row JSON matrix is unchanged within noise because none of its
rows runs a full collection on `main`.

The #10182 pacing half — an old-reclaim trigger bounded by the promoted cohort
— was measured and not included: every variant that produced the intended
regime (a full every two to three parses, peak RSS at or below Node's) cost
+136 to +304 ms of CPU on the target rows, far beyond their CPU lead. A full
over one live 20 MB tree costs ~100 ms even with dead blocks skipped: mark
45 ms, remembered-set rebuild 21 ms, per-live-object sweep accounting 16–19 ms,
census 10–15 ms.
