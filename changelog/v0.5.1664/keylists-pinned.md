Canonical shape and class key-list backings are now allocated old from birth
and excluded from old-page defragmentation and idle compaction. The movement
pin uses the existing shared-layout header bit and does not keep dead lists
alive: full GC still reclaims them.

Remove the canonical trie and class-key memo relocation scanners and their
minor death walks. Static shape-cache minors no longer scan old key lists.
Canonical attributes also use old stationary storage. Nursery string keys request
promotion on their first copy; normal remembered-slot tracing remains for them;
mutable owned tombstone/Array-subclass lists retain their relocation support.

The root-holder census checks the old-allocation, movement-exclusion and
full-prune contract directly, with negative controls for each premise.
Collector tests cover both minor paths, full-GC death, stationary backings
with moving key children, and a pin-removal control through the production
old-page mover shared by idle compaction. Baseline census drift is reconciled
without suppressing deleted holders.

The frozen-main n=5 A/B passes the requested instruction and GC-pause limits
on 14 real programs and four class micros, at both micro iteration counts.
The largest instruction delta is schema-record +0.456%; copying classification
and remembered coverage account for its remaining cost. Fastify still performs
29 to 42 minors after old key birth leaves the nursery cap at 4 MiB, but the
root-walk fix makes instructions and pause flat without changing pacing.
All 18 moving-GC stress outputs match Node, with 24,525 copying minors exercised.
Both caches pay in deletion probes and remain; owned mutable lists and prototype
words still require the residual shape scanner. The matched runtime suite
reproduces seven existing main failures, and the native root corpus reproduces
two main findings without weaker budgets. Detailed tables, RSS increases,
shared-runtime text growth and attribution are in `lane-keylists/report.md` (removed in #13193; `git show 44fcf437dd:lane-keylists/report.md`).

Copying root walks now retain the producer-decoded word and publish diagnostic
provenance once per producer walk. Alignment, header validation, handle rejection,
native-slot context and representation-preserving rewrites remain. This removes
per-slot decode and TLS work without changing nursery policy or adding state.
