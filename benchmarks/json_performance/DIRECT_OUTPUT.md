# Direct final output for strings and small plain objects

Escape-free heap strings can be quoted straight into the final managed
string. Plain objects with at most four inline primitive fields can likewise
use a bounded stack plan containing only lengths and formatted scalar bytes.
The plan contains no managed pointers. The input is rooted across the single
output allocation, and keys/values are re-read afterward before copying.

The object guard verifies tracked ownership, type, allocation/slot bounds,
descriptors, key order and the absence of `toJSON`. Class instances, overflow
fields, complex values and escaped strings keep the general path. This also
preserves observable replacers, spacer coercions, getters and prototypes.
Raw malformed string tails that would change the existing length counter's
interpretation after appending a quote are declined. There is no retained
JSON scratch for successful direct output. GC sources and parse-boundary
hooks remain unchanged from v0.5.1520.

## Measured progress and remaining regressions

Quiet Apple M1; Node 26.5.1, Bun 1.3.14; default GC. The reference remains
the Unicode checkpoint `cb8d8f251`, so the gains include the intervening
scalar and primitive-array changes. All 152 output checks, 456 timing trials
and 432 memory trials complete; the quiet gate passes and the process
monitor observes no external compiler or benchmark.

| Stringify workload | Unicode reference | Direct output candidate | Improvement |
|---|---:|---:|---:|
| Empty object | 136 ns | 52 ns | 2.62× |
| Tiny object | 197 ns | 142 ns | 1.39× |
| 1 KB object | 387 ns | 292 ns | 1.32× |
| 1 MB ASCII-string object | 205 µs | 173 µs | 1.18× |
| Unicode-string object | 255 µs | 137 µs | 1.85× |

The large-string fixtures are objects containing strings, rather than scalar
roots. The flat-object helper is what removes their scratch-buffer copy.
The [complete matrix](results/direct-string/tables.md) includes both parse
and stringify. The [target inventory](results/direct-string/parity.md) is
still 9/38 CPU, 58/74 peak RSS and 28/36 retained RSS medians at or below the
better engine. These counts do not establish acceptance.

**No-regression acceptance remains unmet.** Seven-process repeats confirm
numeric-array parse is 17.6% slower, heterogeneous-array parse 3.2% slower,
8 MB record-array parse 2.1% slower, and escaped stringify 2.4% slower than
the Unicode reference. Their timing ranges are separated. Small-record
stringify remains approximately level with that reference. Long-ASCII
stringify retains a 14.4% repeat improvement, but takes 175 µs versus Bun's
98 µs and Node's 106 µs. The earlier numeric-parse RSS increase is absent in
this build: 64.375 versus 64.531 MiB at 96 calls. Its underlying retention
mechanism has not been fixed or identified.

The numeric-parse slowdown accompanies nearly identical process instruction
counts and about 17% more cycles. The tape builder's hot instruction sequence
is unchanged after normalizing relocated addresses. This suggests a layout or
microarchitecture effect but does not establish its cause. The byte-by-byte
tape scanner is the next parse target. See the [repeats](results/direct-string/recheck-direct/summary.json),
[profiles](results/direct-string/direct-profiles/README.md), and
[disassembly comparison](results/direct-string/disassembly/README.md).

Long-string stringify also has substantial output-allocation/collection
cost: 831 of 2,245 main-thread samples in a separate sustained capture descend
into full collection. Eliminating JSON scratch alone does not remove that
cost. On tiny parse, [separate profiles of the preceding worker](results/direct-string/tiny-parse-profiles/README.md)
put about 81% of `null` samples in trigger bookkeeping, supporting a separate
allocation-free scalar parse path while retaining collection obligations for
allocating parses.

## Validation

60 JSON runtime tests and 28 compiled fixtures pass. New cases cover direct
string/object output, UTF-16 lengths, malformed raw tails, numeric spellings,
retained values, descriptors, key ordering, overflow, prototype changes,
replacers, spacer coercions and reentrant callbacks. Seven retained-output
subjects pass moving-GC stress at seeds 17/9013 with from-space protection,
evacuation verification, and positive collection/movement counters in all
14 runs. Formatting and the root-holder inventory pass.

The same pinned application object and matched runtime/stdlib release
settings are used. [Source/archive provenance](results/direct-string/provenance.json)
and [validation records](results/direct-string/validation.json) identify the
tested candidate. Execution is validated on ARM macOS; x86 execution and full
CPU/RSS parity remain open. This is a development checkpoint, not a completed
performance acceptance result.
