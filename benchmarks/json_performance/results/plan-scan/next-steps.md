Keep all 38 CPU / 74 peak RSS / 36 retained RSS targets and no-regression
acceptance open. Preserve the recovered small-object parse and small-record
stringify gains in later comparisons.

1. Recover escaped/wide stringify CPU (+1.55%/+0.99% versus escaped-count) and
   the numeric parse concern. Instructions are effectively flat for these
   local cases; inspect emitted code/call layout and profile actual execution.
   Do not call unchanged source or flat instruction counts proof of no regression.
2. Include all current rows in the next A/B, plus escaped-count regression
   anchors and the older empty-parse, parse-entry and tape-owned anchors. Retain
   the +0.09375 MiB empty-output RSS, +0.015625 MiB inline-string stringify peak
   and +11.265625 MiB older array-parse peak regressions as requirements.
3. Use large-record-profile/ and gc-coordination.md to diagnose the 20 MB cliff.
   The quiet profile points mostly at GC reached from the benchmark loop.
   Keep GC policy unchanged here and preserve the full CPU measurement; also
   inspect serializer work in stringify_shape_template and toJSON probes.
4. Continue general object parsing and large-record serialization work. The
   current small-record CPU remains 2.61x Node, and overall CPU parity remains
   12/38. Correctness, moving-GC and full RSS coverage remain required.
