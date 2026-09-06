Empty ordinary-object parsing validates the complete input before its final
allocation. It recognizes all JSON whitespace positions and rejects trailing
material. Ordinary and fallible entries use the same leaf. The final object
has a null keys edge and a valid zero-slot shape, preserves Object.prototype
behavior and is explicitly marked plain ordinary. Nested objects retain the
existing general parser. No input pointer is used after the pending hook.

The leaf allocates only its returned object: no managed scratch, temporary
parse roots or suppression/rebaseline cycle. It preserves pending parse debt,
outer suppression, oversized key-cache/ring cleanup and pressure scheduling.
GC production policy and thresholds are unchanged from v0.5.1520.

Measured source: 1ebaf1f40eb834dea148461636111acdec14da24. All 56 recorded
source hashes match. 165 JSON runtime tests, the root-holder inventory gate
(existing 147 frontier unchanged), 37 compiled Node comparisons and 32
moving-GC runs pass. Unit witnesses assert actual movement of both input and
output, exact final-object allocation bytes, freshness and ordinary flags.
The compiled fixture checks all whitespace positions, invalid JSON, numeric
key order, inline/overflow mutations, getters, inherited setters, reviver,
50,000 retained objects and 300,000 churn parses against Node. Both stress
seeds move over 139,000 objects while preserving the complete output.

The full matrix passes 152 output checks, 456 timing trials and 432 memory
trials, with a passing quiet gate and 32 clean external observations. A separate
574-trial paired run covers all 38 CPU rows against tape-owned and three
additional parse rows against the older parse-entry anchor. Its gate passes
with 35 clean external observations, including 24 further trials comparing
retained empty outputs across all four engines.

Paired empty-object parse CPU falls 82.08%, from 0.3224685 to 0.0577755
microseconds (5.58x). Its new full-run CPU remains about 2.4x Bun. Inline-string
parse falls 7.18%, recovering the latest tape-owned regression; 1 MB object-root
parse improves 0.78%, with separated observed ranges.

Regressions are explicit: escaped stringify +2.22%, wide-object stringify
+2.14%, numeric parse +1.52%, with separated paired ranges. Other CPU deltas
mostly have overlapping ranges. Empty-parse peak RSS increases 0.375 MiB;
several other rows increase roughly 0.17-0.23 MiB. The complete ranges are in
recheck-empty-parse/summary.json. The 8 MB array-parse peak remains 11.375 MiB
above the older parse-entry anchor.

Retaining 200,000 empty objects uses 24.859375 MiB current RSS, versus Node
83.421875 and Bun 50.1875. The preceding Perry worker uses 24.6875 MiB, so this
is still a 0.171875 MiB regression against that worker. The additional retained
rows are reported separately and do not change the original target inventory:
11/38 CPU, 58/74 peak RSS and 28/36 retained RSS medians at/below the better
Node/Bun median. All-row and no-regression acceptance remain open.

Local instruction diagnostics are not accepted CPU/RSS measurements. Empty
parse retires 74.66% fewer instructions over the fixed process workload;
other inspected parse paths range roughly -0.01% to +0.30%. The local sample
has 2,197 main-thread samples: 954 enter the final allocator, 575 of these
enter shape publication; 546 enter the pressure scheduling hook. This points
to repeated keyless shape publication and the existing pressure query as
remaining costs, not an exact CPU percentage. The image __TEXT segment has
the same mapped size as the reference, despite about 5.6 KiB more instruction
bytes. This does not explain or dismiss any RSS change.
