### Performance

- **String relational comparison takes an ASCII fast path.**
  `utf16_cmp_bytes` (`crates/perry-runtime/src/string/compare.rs`) — the shared
  core behind `js_string_compare`, `js_string_compare_value`, the string arm of
  `js_jsvalue_compare`, and the default (no-comparator) `Array.prototype.sort`
  key order — ran `str::from_utf8` validation **plus** a scalar `encode_utf16`
  decoder iterator on **both operands, on every call**. When both payloads are
  pure ASCII it now returns `a.cmp(b)` directly.

  This is exact, not an approximation: for bytes `0x00..=0x7F` the UTF-8 byte
  *is* the UTF-16 code unit (zero-extended), so lexicographic byte order and
  lexicographic UTF-16 code-unit order are the same total order — the
  proper-prefix tie-break included, since `<[u8]>::cmp` and `Iterator::cmp`
  rank it `Less` alike. Mixed operands (one ASCII, one not) fall through to the
  unchanged general path, which still compares by UTF-16 code unit so
  astral-vs-BMP ordering (`"\u{FFFD}" > "\u{10000}"`) is unaffected.

  **The precondition is checked, never assumed.** Perry heap-string payloads
  are not guaranteed valid UTF-8 (WTF-8 lone surrogates, `Buffer.toString` of
  arbitrary bytes, FFI blobs — #6085), so the ASCII test has to be total over
  arbitrary byte strings. `<[u8]>::is_ascii` inspects the actual bytes
  word-at-a-time and qualifies. The header-cached predicate `is_ascii_string`
  (`utf16_len == byte_len`) is cheaper but was **rejected as unsound**:
  `compute_utf16_len_wtf8` charges a truncated multi-byte lead its full nominal
  unit count while the payload holds fewer bytes, so `[0xC3]` records
  `utf16_len == 1 == byte_len` and `[0xF0, 0x41]` records
  `utf16_len == 2 == byte_len` — both non-ASCII payloads that the cached
  predicate would call ASCII. A regression test pins those two counterexamples
  so the "cheaper" flag can't be substituted later. (Three of the five call
  sites have no `StringHeader` to read a flag from anyway: SSO stack scratch,
  the owned decimal buffer in `js_string_compare_value`, and the `Vec<String>`
  sort keys in `array/sort.rs`.)

  Measured on a Raspberry Pi 5 (`perf stat -e instructions,cycles`, ASLR off
  via `setarch -R`, pinned to core 2, load-gated, 21 interleaved reps per arm,
  one `CARGO_TARGET_DIR` per arm with the two `perry` binaries and
  `libperry_runtime.a` hash-asserted distinct) — median instructions retired:

  | workload | base | fix | Δ instructions | Δ cycles |
  |---|---|---|---|---|
  | `benchmarks/app-patterns/kernels/batch.ts` | 3,310,641,606 | 3,235,176,619 | **−2.28%** | −1.60% |
  | string-sort microbenchmark | 24,990,715,963 | 22,284,283,749 | **−10.83%** | −10.69% |
  | inert control (no string comparison) | 1,922,098 | 1,923,227 | +0.06% | +0.18% |

  Run-to-run spread was 0.04–0.05% on `batch` and 0.08–0.10% on the
  microbenchmark, so the deltas are 25–130× the noise; the inert control moves
  by less than its own 0.13% spread, which is what rules out cross-arm
  contamination. The flat profile agrees: on `batch`,
  `core::str::converts::from_utf8` (2.33%) + `utf16_cmp_bytes` (1.03%) +
  `memcmp` (0.07%) = 3.43% of samples at base collapses to 0.82% at fix.

  Correctness is guarded by a 45-payload corpus (pure ASCII, embedded NUL, the
  `0x7F`/`0x80` boundary, 2- and 3-byte BMP forms, the `E000..FFFF` band,
  astral-vs-BMP, WTF-8 lone surrogates, truncated multi-byte leads) checked
  **pairwise** against an independently written reference implementation, with
  both arms asserted live so a green run cannot come from never entering the
  fast path, plus antisymmetry over every pair.
