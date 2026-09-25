`crypto.randomUUID()` no longer makes a `getrandom` system call per UUID (#10523). perry-uuid now draws UUID bytes from a per-thread cache refilled once per 128 UUIDs, the batching Node uses, so 120,000 UUIDs cost 942 `getrandom` calls instead of 120,004. `randomUUID({ disableEntropyCache: true })` still draws fresh OS entropy per call, as in Node. UUIDs are formatted into a fixed 36-byte buffer instead of a heap `String`, and v7 UUIDs share the cache.

The global Web Crypto path loses two per-call costs. `crypto.randomUUID()` on `globalThis.crypto` calls stdlib's crypto dispatcher directly after its brand check instead of re-resolving `randomUUID` by name through `js_native_call_method`. Reading `crypto.randomUUID`, `crypto.getRandomValues` or `crypto.subtle`'s KEM methods no longer re-registers the singleton closure's arity (which invalidated its cached call strategy), allocates a fresh `name` string and reinstalls its descriptors on every access; that now happens once, when the singleton is minted. Native-namespace member reads also skip building the user-override probe key when no member has been overridden.

Measured with the issue's benchmark (N = 300,000, median of 3, Linux x64, `PERRY_NO_AUTO_OPTIMIZE=1`; instructions per iteration from callgrind, user space only):

| variant | before ms | after ms | before instr/iter | after instr/iter |
|---|---:|---:|---:|---:|
| `node:crypto` `randomUUID()` | 231 | 74 | 1,670 + 1 syscall | 1,070 |
| hoisted `crypto.randomUUID.bind(crypto)()` | 519 | 126 | 7,159 + 1 syscall | 2,411 |
| uuid `v4()` shape | 777 | 357 | 12,874 + 1 syscall | 8,666 |
| `crypto.randomUUID` read only | 467 | 298 | 11,323 | 7,745 |

Node 22 on the same host: 82 / 86 / 96 / 8.4 ms. The remaining uuid-shape gap is the generic `globalThis.crypto` and native-namespace property read, not UUID generation.

Tests: perry-uuid unit tests (one refill per 128 cached UUIDs, the uncached path bypasses the pool, uniqueness across refills), a perry-runtime unit test that a repeat Web Crypto method read returns the same singleton without redecorating it, and `test_gap_10523_random_uuid_entropy_cache` (every entry point interleaved across several refills; byte-identical to Node). No version bump.
