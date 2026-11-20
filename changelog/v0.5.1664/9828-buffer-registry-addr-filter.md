**The buffer-registry probe stops answering "maybe" to three quarters of the
addresses it is asked about** (#9828).

`is_registered_buffer` guards its three registries with
`BUFFER_LIKE_ADDR_WINDOW`, a process-global min/max span, and the 98.0 %
rejection rate in its doc comment is measured on `claude-code --help` — a run
that registers **10** buffers. A streaming turn registers **213**, scattered
across a **527 MB** span, so `[lo, hi]` covers half a gigabyte of ordinary heap
and stops discriminating: on one 400-character reply, 34.6 million probes, of
which the window admits **73.63 %** to the out-of-line lookup, and **99.79 % of
those find nothing**.

The probe now consults `RegistryAddrFilter` behind the window — the set filter
added after #9272 for exactly this failure, where a registry's entries are
ordinary heap objects interleaved with everything else. Rejection goes from
26.37 % to **96.46 %**, removing **24.25 million out-of-line calls per reply**,
each of which cost a thread-local resolution and a hash. True positives are
unchanged.

The saturation question that structure demands was answered before adopting it:
`RegistryAddrFilter` accrues bits per admission and never clears them, so a
high-churn set would degrade it into the state #9807 documented for the
per-object layout filter. Buffers are the opposite case — probing is hot,
registration is rare — and 213 cumulative admissions against 1,024 bits gives a
10.0 % false-positive rate. `PERRY_BUFFER_DIAG` reports the occupancy, the
window bounds and the rejection rate so the question stays answerable.

In the profile, `is_registered_buffer_slow` falls from 169 to 25 leaf samples
(−85 %); its inline caller rises 96 to 123 as the filter's hashes move there,
so the pair falls 44 % overall. That is roughly half of the 3.19 % the profile
attributed to the slow path, and it is below the streaming rig's resolution, so
turn CPU is unchanged.
