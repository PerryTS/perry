The follow-up is a separate quiet window using the same immutable binaries,
canonical argv[0], CWD and fixture paths. It has 56 untraced timings and four
separate traced processes. Seven repeats use 20 million iterations for null
and string_a parsing, 2 million for small_record, and the original 82 for
escaped_1m. All five observations and entry/exit gates pass.

The longer scalar loops confirm the regression: null +2.599%, string_a +2.108%,
both with separated observed ranges. Small-record parse is +0.216% with
ranges overlapping. Escaped parse remains 1.009% faster with an 832 KiB
peak-RSS increase. These rows are not pooled into or substituted for the
full 38-row comparison.

All four GC traces have one full collection and 20,777 pointer slots scanned.
The total allocated live bytes before collection are identical (34,442,896).
One candidate trace finds 14 conservative roots instead of 13, leaving
2,977,784 live allocated bytes rather than 2,212,752 and five arena blocks
rather than four. The other candidate trace matches the reference root and
live-byte counts while process peak RSS remains higher. Therefore conservative
retention is a lead, not a complete causal explanation of the RSS increase.
The scanner itself introduces no allocator calls or persistent pointer holder.
Do not change conservative-scan policy based on this experiment.
