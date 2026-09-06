# Vectorized Unicode validation and length counting

Long non-ASCII JSON strings previously used standard UTF-8 validation followed
by code-point decoding to compute the output string's UTF-16 length. The shared
string allocator now sends inputs of at least 64 bytes through `simdutf8` 0.1.5
and a bounded NEON/SSE2 length counter. The validator was already in the lockfile;
it is now a direct runtime dependency. ASCII and short-string counting retain
their existing paths. Invalid UTF-8 still uses the existing WTF-8 counter.

The large-string helper stays outside the inlined allocation path. Its work
uses stack storage, with no managed allocations, callbacks or retained state.
GC sources, collection thresholds, and JSON parse-boundary hooks are unchanged.

## Measured results

The [full matrix](results/unicode/tables.md) compares this candidate with the
cache-bounded runtime at `c559a125c`. Both use the same application object,
matched runtime build settings and default GC. Medians come from three fresh
processes on the quiet Apple M1, alongside Node 26.5.1 and Bun 1.3.14.

| Unicode fixture | Previous Perry | Candidate Perry | Node | Bun |
|---|---:|---:|---:|---:|
| Parse CPU per call | 0.984 ms | 0.257 ms | 0.438 ms | 0.059 ms |
| Stringify CPU per call | 0.982 ms | 0.256 ms | 0.420 ms | 0.451 ms |
| Parse peak RSS | 56.47 MiB | 55.61 MiB | 164.88 MiB | 123.14 MiB |
| Stringify peak RSS | 60.02 MiB | 58.33 MiB | 138.53 MiB | 97.23 MiB |

Both operations improve **3.8×**. Perry now beats Node for this parse case and
both engines for stringify; Bun's parse remains 4.35× faster. Other CPU medians
are mostly within about 1.5% of the reference. Across all measured rows, no
candidate peak/current RSS median increases by more than 1 MiB. RSS remains
whole-process memory, not a measure of temporary allocations inside JSON.

**This candidate does not meet the no-regression requirement.** Null and `"a"`
stringify take roughly 4% more CPU, about 2.6–2.9 ns per call. The
[seven-process repeats](results/unicode/recheck-scalars/) preserve the follow-up
measurements. These regressions must be resolved before performance acceptance.
The [complete inventory](results/unicode/parity.md) has only 6/38 CPU medians,
58/74 peak RSS medians and 28/36 retained RSS medians meeting the better engine.
Those counts are not a statistical or correctness completion gate.

## Evidence and remaining work

The [initial count-only candidate](results/utf16/README.md) improved Unicode
by only 20%. Its profiles place about 78% of samples in standard UTF-8
validation, motivating the second change. Its exact source patch, measurements
and confirmed tiny-call regression are preserved separately.

Validation for the combined candidate:

- 100 string-related runtime tests and 47 JSON tests pass. Coverage includes
  all Unicode scalar values, SIMD tails and unaligned slices, protected-page
  boundaries, lone surrogates, malformed bytes, and byte-mutation comparisons
  with standard validation plus the existing fallback.
- 24 compiled fixtures match Node, including Unicode lengths, serialized code
  units, and retained output graphs. All 152 benchmark output checks pass;
  456 timing trials and 432 memory trials complete with a passing quiet gate.
- Scanning, retained-wide-output and Unicode fixtures pass GC stress seeds
  17/9013 with from-space protection and evacuation verification. All six runs
  have positive copying-collection and moved-object counters.
- `python3 check_utf16.py` imports the production counter and checks 33,793
  valid/malformed-input cases, including a string containing every Unicode
  scalar. A counting allocator observes zero temporary allocations.
- The production SSE2 counter and validator compile for x86-64 Linux; they
  have not been executed locally on x86. Reported performance is ARM-only.

[Validation](results/unicode/validation.json), [allocation checks](results/unicode/allocation-check.json),
[source/archive provenance](results/unicode/provenance.json), and the
[quiet window](results/unicode/window.json) provide the records.

The next work remains tiny-call overhead, compact decimal formatting and
general object traversal. The captured wide-object profile still spends much
of its time in collection after the cache bound; vectorized text processing
alone cannot close that gap. Full CPU/RSS parity and the no-regression goal
remain open.
