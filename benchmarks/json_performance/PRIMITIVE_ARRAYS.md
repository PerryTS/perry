# Primitive-array stringify without recursive traversal

Dense arrays containing only primitive values can serialize directly from
their element storage. The existing array entry resolves forwarding and
applies `toJSON` first. The new helper verifies allocator ownership and the
array type, declines descriptor/named-property cases, and checks every element
before writing. A complex value falls back with the output and serializer
state untouched. Numeric arrays use the existing raw-f64 layout proof and
avoid that validation pass.

The successful path cannot recurse, invoke a callback or allocate managed
scratch, so it needs no temporary GC roots or circular-reference stack entry.
String escaping and number spelling use the existing emitters. Output-buffer
growth uses Rust allocation; it cannot reach a Perry GC safepoint. Existing
GC policy, parse-boundary hooks and input/output layouts are unchanged.

## Measured results

Medians of three fresh processes on the quiet Apple M1, with Node 26.5.1,
Bun 1.3.14, identical application object code and default GC. The reference is
the Unicode runtime `cb8d8f251`, so these gains include the intervening scalar
change as well as the new array traversal. CPU per stringify call:

| Fixture | Unicode reference | Candidate | Node | Bun |
|---|---:|---:|---:|---:|
| Small record | 551.321 ns | 537.592 ns | 108.137 ns | 120.410 ns |
| 16 KB record array | 35.908 µs | 31.249 µs | 13.173 µs | 23.206 µs |
| 1 MB record envelope | 2.207 ms | 1.974 ms | 0.849 ms | 0.968 ms |
| 8 MB record envelope | 18.646 ms | 16.691 ms | 6.781 ms | 8.455 ms |
| Numeric array | 6.639 ms | 1.607 ms | 1.980 ms | 2.953 ms |

Numeric stringify improves 4.13× over the Unicode reference and beats Node
by 1.23× and Bun by 1.84×. Record stringify at 16 KB–8 MB improves about
12–15%. The [full matrix](results/primitive-array/tables.md) preserves both
operations and every memory row. [All targets](results/primitive-array/parity.md)
show 9/38 CPU, 58/74 peak RSS and 28/36 retained RSS medians at or below the
better engine; median counts alone do not establish acceptance.

**The no-regression requirement is still unmet.**
[Seven-process checks](results/primitive-array/recheck-primitive/summary.json)
show that the preceding scalar checkpoint's numeric-parse and escaped-string
CPU regressions are recovered, with overlapping reference/candidate ranges.
Small-record stringify is now 2.5% faster, with separated ranges. However,
wide-object parse has a 2.8% slower median, long-ASCII stringify a 1.1% slower
median, and numeric-array parse peak RSS is 66.78 versus 64.53 MiB. The wide
and ASCII timing ranges overlap partially; these remain concerns to resolve,
not a claim of no regression. General-object CPU/RSS parity remains open.

For ordinary materialized objects, the remaining CPU gaps depend on size:

| Fixture / operation | Perry | Node | Bun | Node advantage | Bun advantage |
|---|---:|---:|---:|---:|---:|
| Small record / parse | 0.750 µs | 0.339 µs | 0.253 µs | 2.21× | 2.96× |
| Small record / stringify | 0.538 µs | 0.108 µs | 0.120 µs | 4.97× | 4.46× |
| 1 MB record envelope / parse | 4.034 ms | 2.754 ms | 2.137 ms | 1.46× | 1.89× |
| 1 MB record envelope / stringify | 1.974 ms | 0.849 ms | 0.968 ms | 2.32× | 2.04× |
| 8 MB record envelope / parse | 31.687 ms | 29.193 ms | 21.903 ms | 1.09× | 1.45× |
| 8 MB record envelope / stringify | 16.691 ms | 6.781 ms | 8.455 ms | 2.46× | 1.97× |
| 20 MB record envelope / stringify | 94.589 ms | 17.369 ms | 20.812 ms | 5.45× | 4.54× |

The large-array parse wins in the full matrix can use Perry's lazy output;
they do not establish equivalent fully materialized parse throughput. The
20 MB stringify row includes substantial collection cost in the measured
loop. Peak RSS for 1 MB record stringify is 60.31 / 108.52 / 100.27 MiB
(Perry / Node / Bun), while 8 MB object parse is 231.59 / 225.52 / 111.28 MiB.
Perry's memory advantage therefore also depends on the workload.

[Diagnostic GC runs](results/primitive-array/gc-comparison/README.md) show
different surviving arena blocks in numeric parse. A
[28-process control](results/primitive-array/argv-control/README.md) rules out
executable-name length as their cause. The RSS regression remains unresolved.

## Validation and provenance

- 54 JSON runtime tests pass. New tests cover rejection of small/large buffer
  receivers, unchanged output and capacity on a complex-array fallback, and
  primitive values including WTF-8 surrogate strings.
- 26 compiled fixtures match Node. The new array fixture exercises numeric
  layout changes, undefined/holes, retained output, subnormal values, Unicode,
  array and object `toJSON`, reentrant stringify and cycles.
- All 152 benchmark output checks pass. The full 456 timing and 432 memory
  trials pass the quiet gate, with no external compiler/benchmark observed.
  The seven-process follow-up window also passes the quiet gate.
- Five retained-output fixtures pass moving-GC stress at seeds 17/9013, with
  from-space protection and evacuation verification. All ten executions have
  positive copying-collection and moved-object counters.
- Formatting and the runtime root-holder inventory pass. The numeric emitter
  itself is unchanged from the [1,730,283-value scalar checks](SCALARS.md).
  This candidate has been executed on ARM macOS; x86 execution remains open.

[Validation](results/primitive-array/validation.json),
[source/archive provenance](results/primitive-array/provenance.json), and the
[quiet window](results/primitive-array/window.json) identify the evidence.
The runtime/stdlib archives were rebuilt together with matching release
settings; the application object comes from the same pinned development
compiler used throughout the study. Full goal completion is not claimed.
