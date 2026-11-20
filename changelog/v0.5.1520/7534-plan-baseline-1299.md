### Docs: engine plan carries the full v0.5.1299 baseline, and a re-ordered backlog

The plan's performance backlog was materially stale: its worst row was
`json_parse_1mb` at 6.27x, and it did not list `object_deep_clone` or
`promise_all_chains` at all — because until #7495 and #7516/#7529 landed, those
two kernels **crashed** rather than ran.

Replaced with all twelve app-pattern kernels measured on the pinned quiet mini
at v0.5.1299 (AC power, CPU-quiet gate, node 22.23.1 / bun 1.3.14, 11 runs per
cell), plus the JSON polyglot legs. Two findings drive a re-ordering:

- **`object_deep_clone` is 37.5x bun** (657 ms vs 17.5 ms) — the worst cell by
  3x over the next, and never before measurable. Now #7533, and first in the
  queue with a profile-first instruction.
- **The JSON lazy tape is a net negative on scans**: the optimized build is
  2.2x slower than the unoptimized one on `field_access` (2984 ms vs 1350 ms)
  with 3.6x the RSS, while remaining a 6.8x win on `roundtrip` (192 ms, beating
  bun's 216 and node's 379 and within ~8% of Rust serde_json). #7478.

`#7502` (nine shipped-lowering mechanics with no assertion anywhere) is added
to the ordering explicitly: today's ~20 rooting bugs were all found by hand
with `PERRY_GC_PROTECT_FROMSPACE`, because nothing else can find them.
