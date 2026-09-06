Bounded inline-object parsing decodes before collection and allocates only a fresh final object. Paired tiny-object parse CPU drops 67.04% to 0.1196715 microseconds; escaped stringify and array parse regressions remain. Full/854-trial paired matrices qualify, alongside 182 runtime tests, 39 compiled comparisons, 36 moving-GC runs and 63 hashes. All-row/no-regression acceptance remains open. See benchmarks/json_performance/results/inline-object/README.md.

Latest marker-probe checkpoint reduces paired tiny/16 KB array/1-8 MB record stringify CPU by 3.10%/4.36%/2.36-2.78% versus plan-scan, but introduces small-object parse and whole-process RSS regressions. All-row and no-regression acceptance remain open (12/38 CPU, 58/74 peak, 28/36 retained). 176 runtime tests, 38 compiled comparisons, 34 moving-GC runs and complete qualified full/784-trial paired matrices pass. See benchmarks/json_performance/results/marker-probe/README.md for exact tradeoffs and earlier requirements.

Accelerate JSON parse and stringify with bounded scanners, vectorized Unicode
and escape counts, duplicate-key indexing, bounded shape retention, exact
number formatting and direct final output for eligible strings and records.
Large native tapes transfer into exact lazy-result storage. Empty ordinary
objects validate their complete input before final allocation. Short-word
scanning is confined to scalar output plans, whose initialized placeholders
have smaller active payloads.

Root parse and direct-output inputs before collection and rederive moved
pointers. Preserve array getter order, lazy assignment aliases, descriptors,
callbacks, malformed-string behavior and retained-output identity. GC production
policy, thresholds and gc_bump_malloc_trigger remain unchanged from v0.5.1520;
GC-directory changes are tests only. No version bump.

The latest source passes 172 runtime tests, 38 pinned compiled comparisons,
34 moving-GC runs and 59 source hashes. Full CPU/RSS and 756-trial paired
matrices qualify. Small-record stringify improves 10.47% versus escaped-count,
with peak RSS down 0.078125 MiB; small-object parse regressions are recovered.
Acceptance remains open at 12/38 CPU, 58/74 peak and 28/36 retained RSS. Escaped
and wide stringify regress 1.55%/0.99%, and all other unresolved CPU/RSS
regressions are recorded. A quiet 20 MB profile points to substantial loop GC
work; it is diagnostic, not a measured GC CPU percentage. Full results and
rejected experiments are in benchmarks/json_performance/DATA_RECORDS.md.
