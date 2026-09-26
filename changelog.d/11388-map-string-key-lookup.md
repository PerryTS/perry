perf(map): string-keyed `Map` lookups no longer pay the generic comparison per rejected entry (#10697).

A string key reached every candidate entry through `jsvalue_eq`, which on each
**mismatch** re-probed both operands for symbol-ness and bigint-ness,
re-validated both GC headers and decoded both inline (SSO) strings byte by byte
before a length compare could reject the pair. A lookup's cost was therefore set
by how many entries it rejected before its match. That is why `m.get("alpha")`
against the first entry measured 2.2× cheaper than `m.get(CATS[i & 3])` spread
over four entries, and why the "count by a few constant categories" shape was
perry's worst `Map` case, at 4.2× node.

- **Identity pre-scan** (`find_identical_key`, small maps ≤ 8 entries): a Map
  never holds two SameValueZero-equal keys, so an entry whose bits equal the
  key's is the match. The common shape looks keys up with the same value they
  were inserted with. That value is read from the same array, or is the same
  literal, so the match is found at a few instructions per entry before any
  content comparison runs.
- **String lane** (`map/string_key.rs`, `StrKey`): an SSO or `STRING_TAG` key
  is decoded once per lookup. Each entry is then rejected by its tag and byte
  length before its bytes are read. Two inline strings compare as one masked
  word. Two heap strings compare their first and last 8-byte words inline
  before `memcmp`, which covers every key up to 16 bytes and handles shared
  namespace prefixes (`category-alpha` / `category-gamma`). The hashed index
  used by larger maps hashes the decoded key and validates candidates through
  the same lane.
- Pointer-tagged and raw-pointer keys are deliberately not decoded. A
  description-less `Symbol()` exposes a zero-length string view, and only
  `jsvalue_eq`'s symbol guard keeps it apart from the `""` key (#4570). Such
  keys, and every non-string key, keep the generic path unchanged.

Measured as instructions per operation, fitted from N=100k to N=1M with
callgrind, `perry-dev` runtime on x86-64. Output is identical to node on every
row.

| shape | before | after |
|---|---|---|
| `m.get(CATS[i & 3])`, 4 keys pre-populated | 587 | 315 |
| count-by (`m.set(k, (m.get(k) ?? 0) + 1)`), 4 short keys | 968 | 427 |
| count-by, 4 keys sharing a `category-` prefix | 968 | 427 |
| 1,024 distinct keys (hashed index) | 624 | 601 |

On the first row, 178 of the remaining 315 instructions are the loop body in
`main` (the array index and the arithmetic), not the `Map`.

Template literals (the issue's second item) are not addressed here.
