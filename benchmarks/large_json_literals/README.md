# Large JSON literal lowering (#10151 / #10161)

`generate.py` reproduces the typed record shape and 2,000-element numeric table
from the #10161 performance audit. `hot.ts` runs 20,000 passes over each: numeric
array indexing, then `q.w + q.tags.length + q.id` on 400 records. The separate
`records-N.ts` files scale the same record shape to locate the LLVM codegen cliff.
`numbers.ts` and `records.ts` run those same loops in separate entrypoints. The
generated fixtures are deliberately outside the repository.

Run on a Linux build host with LLVM 22 and a release compiler plus matching
runtime/stdlib archives. In the issue lane, every command below is passed through
`./remote.sh`; do not build or run the probes on the Mac.

```sh
python3 benchmarks/large_json_literals/generate.py /tmp/literal-probes
cd /tmp/literal-probes
# Avoid inheriting unrelated define settings from a parent directory.
printf '{}\n' > perry.json
export PERRY_RUNTIME_DIR=/path/to/pinned/compiler-and-archives
PERRY_BIN=$PERRY_RUNTIME_DIR/perry
/usr/bin/time -v timeout 600 env PERRY_CODEGEN_PROGRESS=all "$PERRY_BIN" compile hot.ts \
  --no-auto-optimize --output ./hot --cache-dir cache-hot
./hot
/usr/bin/time -v timeout 600 env PERRY_CODEGEN_PROGRESS=all "$PERRY_BIN" compile records-4800.ts \
  --no-link --no-auto-optimize --output records-4800.o --cache-dir cache-records-4800
```

For an A/B, compile with each pinned compiler to separate outputs and fresh cache
directories, then run the executables in alternating order five times. Compare
both checksums as well as median `table_ms` / `recs_ms`. Repeat the no-link command
for the other record counts. The 600-second timeout bounds an intentionally
pathological **ordinary-lowering** measurement; it is not a CI performance test.

The compact path has two tiers. Flat primitive arrays qualify at 1,024 AST value
nodes or 64 KiB of UTF-8 string content. Records and nested arrays have a much
higher threshold of 24,576 nodes or 1 MiB of UTF-8 key/string content. This
preserves ordinary lowering's static record shapes
for mid-size hot data; JSON-parsed records otherwise pay the generic property IC
cost. The cutoff tests live in `perry-hir`, and the timed define/semantics/cache
regressions live in `crates/perry/tests/issue_10151_large_json_define.rs`.

## Linux measurements (LLVM 22.1.8, 2026-09-13)

The ordinary control is the compiler code at `8a058e2053` (the PR's base), built
by removing the compact-lowering hook from this branch. Both variants link the
same freshly built runtime/stdlib archives. Compilers and archives were copied
together to separate directories, away from the shared Cargo target. The
original broad compact rule at `d0bb9b1fd5` was also retained for comparison.

| Record count | Literal source bytes | AST value nodes | Ordinary compile | Compact compile |
| ---: | ---: | ---: | ---: | ---: |
| 4,800 | 307,740 | 33,601 | 475.17 s | 0.35 s |
| 6,400 | 412,540 | 44,801 | >600 s (timeout) | 0.37 s |

These are no-link compiles with fresh caches. At 6,400 records, IR emission
finished in 4.5 seconds, producing about 85.1 MiB of estimated IR and a
1,446,386-instruction function before optimization. Four of five LLVM units
finished in under a second; the remaining unit timed out. The 24,576-node
threshold switches this shape at 3,511 records, 27% below even the 4,800-record
eight-minute case and 45% below the 6,400-record timeout. It is 24 times the
primitive node threshold; the text threshold is 16 times larger.

Five interleaved runs of the final binaries gave these medians:

| Fixture / hot loop | Ordinary control | Two-tier rule |
| --- | ---: | ---: |
| Separate 2,000-number array | 71 ms | 71 ms |
| Separate 400 typed records | 295 ms | 294 ms |
| Combined audit file: numbers | 290 ms | 291 ms |
| Combined audit file: records | 292 ms | 291 ms |

Checksums match (`79042210000` for numbers, `2011000000` for records). The
original broad rule measured 71 ms / 938 ms in the combined file. Restoring
ordinary record lowering removes that record-read regression. However, its
622k-instruction initialization keeps the **combined** unit above the existing
100k-instruction O0 machine-code limit, also slowing its numeric loop. HIR
inspection confirms that the numeric array still uses `JsonParse`. The isolated
number probe shows no intrinsic runtime speedup on this host. Do not describe
the earlier combined-file speedup as an unconditional primitive-array win.

With the final compiler, the preserved OpenCode define (4,623,800 bytes,
213 providers) compiles and links in 1.52 seconds. The refreshed installed
snapshot (4,637,074 bytes, also 213 providers) takes 1.54 seconds. Both print 213.
