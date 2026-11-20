// Memory-leak regression: 200,000 iterations of mixed-length string
// allocation. Targets the SSO landing (v0.5.213-216) — short strings
// (≤5 bytes) are inline NaN-box payloads with zero heap allocation,
// medium strings hit the heap StringHeader path, long strings hit
// the same heap path with larger payloads. RSS must plateau.
//
// Catches regressions where:
//   - SSO accidentally allocates (e.g. a regression to the v0.5.215
//     SSO PropertyGet/.length path where a fast-path miss falls
//     through to an alloc)
//   - String concat materializes an SSO operand to heap unnecessarily
//   - Heap-string GC loses track of strings (MALLOC_STATE leak)
//   - Number-to-string coerce ("a" + (i % 9)) pins per-iter strings

let total = 0;
for (let i = 0; i < 200_000; i++) {
  // SSO range (≤5 bytes after concat)
  const a = "a" + (i % 9);
  const b = "bb" + (i % 9);
  const c = "x";

  // Heap range (definitely > 5 bytes)
  const d = "string_" + i;
  const e = "long_value_" + i + "_with_padding";

  total += a.length + b.length + c.length + d.length + e.length;
}
console.log("done, total=" + total);
