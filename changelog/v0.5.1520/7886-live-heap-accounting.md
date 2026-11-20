### Fixed: `heapUsed` and major-GC pacing counted arena high-water as live memory

`process.memoryUsage().heapUsed`, V8-compatible heap telemetry, and major-GC
pacing used the sum of arena block bump offsets as if it were live allocation.
A tiny survivor therefore charged every dead object below the same block's bump
pointer, while reusable old-generation holes were corrected in public telemetry
but not in pacing.

Completed collections now publish a header-inclusive live-object census.
Between collections, bump growth and general/old free-list consumption advance
that census without adding work to the generated inline allocator. Copying
minors derive the next census from the previous live count minus from-space plus
copied/promoted survivors; full and non-moving collections use their sweep walk.
Reserved capacity and allocation high-water remain available as separate
fragmentation diagnostics.

On the quiet M1 mini, seven-repeat A/B measurements across all 14 GC-ratchet
probes produced byte-identical program output and reduced reported `heapUsed` by
59.2% to 99.99%, while `heapTotal` was unchanged in every probe. For example,
the promoted-then-released large-live-set probe reports 51,457,736 → 4,462,040
bytes (−91.3%). Collector counters and timing were unchanged on 13 probes. The
64 MiB large-Eden probe performed one additional GC step, trading +7.5% wall
time for −5.9% RSS; this is the explicit pacing tradeoff of using live bytes
rather than fragmentation as the major-collection signal.
