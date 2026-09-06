Accelerate JSON parse and stringify with bounded scanners, vectorized Unicode handling, duplicate-key indexing, bounded parse-shape retention, ECMAScript number formatting, inline scalar results, primitive-array/object emission and direct final output for qualifying strings and records. Exact integers below 2^53 use integer formatting in output plans. Large native tapes transfer into lazy-result storage. Empty ordinary objects validate their complete input before a single final allocation, with no managed scratch or parse suppression/rebaseline cycle.

Root general parse inputs before pending collection and rederive moved bytes. End native tape input borrows before collection. Honor later-record descriptors and array index getters. Root direct-output records before allocating prototype lookup or final output. Root lazy computed assignments through materialization and preserve the original alias length. GC production policy, thresholds and gc_bump_malloc_trigger remain unchanged from v0.5.1520; GC-directory changes are tests only. The empty leaf preserves pending debt, outer suppression, key-cache/ring cleanup and pressure scheduling.

The latest measured source passes 165 JSON runtime tests, 37 compiled Node comparisons and 32 moving-GC runs. Its full M1 CPU/RSS matrix, 574-trial paired comparison and 24 extra retained-empty trials pass quiet gates and external monitoring. Against tape-owned, empty-object parse CPU falls 82.08%, from 0.32247 to 0.05778 microseconds (5.58x); inline-string parse improves 7.18%, and 1 MB object-root parse 0.78%.

This remains a development checkpoint: 11/38 CPU, 58/74 peak RSS and 28/36 retained RSS medians meet the better Node/Bun median. Escaped stringify regresses 2.22%, wide stringify 2.14%, and numeric parse 1.52%. Empty-parse peak RSS rises 0.375 MiB; several other rows rise about 0.17-0.23 MiB. Retaining 200,000 empty objects uses 24.86 MiB versus Node 83.42 / Bun 50.19 MiB, but also 0.17 MiB more than preceding Perry. Older unresolved regressions remain documented; no-regression acceptance fails. Array-root parsing may defer materialization; stringify inputs are fully materialized. RSS is whole-process memory. Full results and rejected experiments are in benchmarks/json_performance/DATA_RECORDS.md. No version bump.

The Piece-borrowing experiment was measured and reverted: paired large-array
parse regressed about 5-5.5% and small-record stringify 1.27%. Complete raw,
validation and paired evidence is retained under results/piece-ref.

Validated escaped UTF-8 strings now plan exact output and emit into the final
managed string. ARM counts expansion in bounded vector blocks; other targets
use a scalar table. Invalid UTF-8/WTF-8 retains the general serializer. Paired
escaped stringify CPU falls 49.97% versus empty-parse (about 2x Node / 2.2x Bun
in the full matrix), with peak RSS down 0.84 MiB. 172 runtime tests, 38 compiled
comparisons and 34 moving-GC runs pass. Complete full/paired matrices qualify.
Acceptance remains open at 12/38 CPU, 58/74 peak and 28/36 retained RSS: small
record stringify +1.60%, several small-object parse regressions and all older
unresolved regressions are recorded in results/escaped-count. No GC production
policy or version change.
