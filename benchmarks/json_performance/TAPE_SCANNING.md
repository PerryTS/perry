# Bounded string and digit scanning in the tape builder

The tape builder now skips ordinary string bytes with the existing bounded
NEON/SSE2/word scanner and counts numeric digit runs eight bytes at a time.
Escape validation, required fraction/exponent digits, leading-zero rules,
token offsets and container links keep their existing behavior. The word
scanner normalizes byte order and never loads beyond the supplied slice.

The change creates no managed scratch, allocation or collection point. GC
policy and `json/parse_api.rs` remain byte-identical to v0.5.1520. The tape's
existing scratch/ownership and output-allocation behavior is unchanged.

## Measured result

The full quiet Apple M1 comparison includes 152 output checks, 456 timing
trials and 432 memory trials against the Unicode reference, Node 26.5.1 and
Bun 1.3.14. Default GC is used. All outputs match; the quiet gate passes and
34 process-monitor observations detect no external compiler or benchmark.
Array-root parse may return a lazy Perry value; stringify starts with a fully
materialized value. RSS measures the whole process.

A separate quiet run repeats 13 pairs seven times each (182 timing trials).
The paired parse improvements against the preceding direct-output candidate
are:

| Workload | Direct-output candidate | Tape scanner | CPU reduction |
|---|---:|---:|---:|
| Numeric array | 1.845 ms | 1.716 ms | 6.97% |
| 1 MB record array | 1.576 ms | 1.521 ms | 3.53% |
| 8 MB record array | 14.152 ms | 13.691 ms | 3.26% |

The timing ranges are separated for these three comparisons. The initial
16 KB record-stringify slowdown does not repeat: paired medians are 31.714
versus 31.691 microseconds, with overlapping ranges. Large object-root parse
and small-record stringify repeats also overlap.

**No-regression acceptance remains unmet.** Against the Unicode reference,
seven-process repeats show numeric parse **+9.76%** (1.565 to 1.718 ms),
heterogeneous parse **+2.78%** (1.870 to 1.922 ms), and escaped stringify
**+2.41%** (1.864 to 1.909 ms). All three ranges are separated. The numeric
regression is smaller than the preceding candidate's 17.6%, but unresolved.
Against the direct-output candidate, `"a"` parse is **1.00% slower**, also
with separated ranges. That row does not use the changed tape builder; the
cause has not been established.

Peak RSS also increases modestly in the paired comparison: numeric parse
64.391 to 64.672 MiB, 1 MB record-array parse 62.063 to 62.359 MiB, and tiny
string parse 13.469 to 13.672 MiB. The code adds no managed allocation, but
these measured process-memory increases still belong in the acceptance
record; their cause is not established.

The [complete CPU/RSS inventory](results/tape-scan/parity.md) remains
**9/38 CPU, 58/74 peak RSS and 28/36 retained RSS** medians at or below the
better engine. General-object stringify remains a substantial gap. The
[full matrix](results/tape-scan/tables.md) and
[paired repeats](results/tape-scan/recheck-tape-scan/summary.json) preserve
the remaining targets and regressions.

## Correctness and provenance

- 121 runtime tests selected by `cargo test -p perry-runtime --lib json`
  pass, including exhaustive digit/byte boundary cases and guard pages that
  fault if a scanner reads beyond its input.
- The old and new production tape builders agree on acceptance and every
  token's offset, kind and link for 323,211 valid, truncated and mutated
  inputs. [Reproduction harness](results/tape-scan/tape_diff.py).
- 29 compiled fixtures match Node. The new fixture retains 1,088 parsed
  arrays, checks numeric values (including negative zero, overflow and
  underflow), escaped/Unicode strings and nested values, and rejects 49
  malformed inputs.
- Eight subjects pass moving-GC stress at seeds 17 and 9013 with from-space
  protection and evacuation verification. All 16 runs have matching output
  and positive collection/movement counters.
- Formatting and the root-holder inventory pass. The existing unverified
  inventory frontier is unchanged. The x86-64 leaf harness compiles; execution
  is validated on ARM macOS only.

The same pinned application object is linked against matched runtime/stdlib
release archives. The completed build took 7m28s; pre-build source hashes
match the archived candidate. The worker hash is
`21a1e75beb47e2a12ae066d4991abf7878e48c297b3de55bd5514077f209e7e5`.
[Provenance](results/tape-scan/provenance.json),
[validation](results/tape-scan/validation.json), and both quiet-window records
identify the tested state. This is a development checkpoint; CPU/RSS parity
and no-regression acceptance remain open.
