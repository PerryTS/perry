**Fixed** `bench_fibonacci` and `bench_bitwise` measured nothing on Perry and
reported enormous false wins.

Both discarded the benchmark's return value, so Perry proved the call pure and
eliminated the timed loop entirely — `TOTAL:0`, which wall-clock read as ~240×
faster than Node on fibonacci and *infinitely* faster on bitwise. Node happened
not to make the same elimination, which is the only reason the two disagreed.

Both now accumulate into a sink that is printed as `CHECKSUM:`, so the work
cannot be removed and correctness is verifiable against Node. `bench_fibonacci`
additionally varies its argument per iteration: with a loop-invariant
`fibonacci(FIB_N)` the call hoists out of the loop and runs **once**, keeping the
checksum correct while `TOTAL` drops to 0 — the same false win by a second
mechanism. Both files now assert their subject was live and print a loud `ERROR:`
line if the sink is zero or the timed loop reports no elapsed time.

The corrected picture, checksums matching Node exactly:

| benchmark | was reported | actually |
|---|---|---|
| `bench_fibonacci` | ~240× faster | **2.5× faster** |
| `bench_bitwise` | infinitely faster | **20.4× SLOWER** |

The bitwise result is the finding: tight integer arithmetic (`%`, `*`, `+` in a
10M-iteration loop) is where Perry is furthest behind, and the suite has been
reporting it as an unbounded win.

`run_benchmarks.sh` greps `^TOTAL:` only, so the added lines are compatible.
