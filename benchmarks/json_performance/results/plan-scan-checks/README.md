Follow-up source 7117f9e67d11af140ee857880b9a96af2ed2890e restores the seven
shared scanner function bodies to escaped-count and calls short-word packing
only from the scalar output planner. Initialized String plan seeds remain.

172 JSON runtime tests and the existing root-holder gate pass. The new predicate
is checked against an independent scalar oracle for every byte value, adjacent
byte pair and scanner boundary, including guard-page placement. All 59 current
source hashes are recorded. This directory contains source checks only.

Release build, new compiled-fixture/moving-GC validation and new complete CPU/RSS
comparisons remain pending at this checkpoint. The short-tail CPU/RSS results
belong to source 090e120f3 and do not establish this follow-up's performance.
The next comparison uses escaped-count as the retained reference, plus four
short-tail anchors (small-record stringify, long-ASCII/Unicode parse and 1 MB
record-array parse), alongside all existing reference rows: 54 cases / 756 trials.
All 38 CPU / 74 peak RSS / 36 retained RSS and no-regression requirements remain.

The release and full comparisons subsequently completed. See
[measured plan-scan results](../plan-scan/README.md) for authoritative validation
and performance; the pending status above describes the earlier source-check
checkpoint only.
