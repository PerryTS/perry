**perf(codegen): inline `charCodeAt` and stop routing it through the dynamic bitwise helper (#7592)**

`honest_bench`'s `json_pipeline` finishes by hashing its own 68 MB serialized
output with a hand-rolled FNV-1a loop — `h = (h ^ s.charCodeAt(i)) | 0` over
every character. At 500k records that phase cost **1,207 ms at 17.7 ns/char**,
roughly twice bun's entire run. A leaf profile showed 85% of it was FFI rather
than work: `js_string_char_code_at` 31.5%, `js_dynamic_bitxor` 31.0%,
`js_string_index_to_i32` 13.1%, `js_get_string_pointer_unified` 9.0% — the JS
loop itself was 15.3%. Four opaque runtime calls per character.

Two independent defects, each fixed:

1. **`charCodeAt` was not statically a Number.** `is_numeric_expr` had no arm
   for a String-method call, so `h ^ s.charCodeAt(i)` failed `expr/binary.rs`'s
   "both operands are statically primitive" test and computed an integer xor
   through the BigInt-aware `js_dynamic_bitxor`. The admitted set is exactly
   `charCodeAt`/`indexOf`/`lastIndexOf`/`search`/`localeCompare`, each verified
   against its lowering. `codePointAt` is deliberately excluded — it returns
   `undefined` out of range, a NaN-box *tag*, not a number. The claim is gated
   on the receiver taking codegen's proven-string routing, mirroring
   `lower_call/property_get.rs`'s condition, so an `any`-typed receiver (which
   may be a user object with its own `charCodeAt`) is never claimed.

2. **`charCodeAt` had no inline path.** It has one now: a guard chain that
   reproduces exactly what `js_string_char_code_at` + `js_string_index_to_i32`
   compute — `STRING_TAG` receiver, handle ≥ 4096, `0 <= index < 2^31-1` as
   ORDERED comparisons (so a NaN-boxed index falls back to the full
   `ToIntegerOrInfinity`, and the following `fptosi` can never be poison),
   `utf16_len == byte_len` (the runtime's own `is_ascii_string`, which implies
   every byte < 0x80 so no WTF-8 / lone-surrogate / astral payload reaches the
   byte load), `index < utf16_len` — and falls back to those same two calls for
   anything it cannot prove. Removing the calls is also what lets LICM hoist
   the loop-invariant receiver unbox and header loads, which an opaque call on
   the critical path had been blocking.

Measured on the pinned quiet host, both arms built from one target dir with an
identical package set and run interleaved, output hash byte-identical on every
row:

| | fnv1a phase | ns/char | peak RSS |
|---|--:|--:|--:|
| 200k records, before | 483 ms | 17.7 | 598.7 MB |
| 200k records, after | **43 ms** | 1.6 | 598.7 MB |
| 500k records, before | 1,247 ms | 17.7 | 1,389.2 MB |
| 500k records, after | **111 ms** | 1.6 | 1,389.2 MB |

**11.2x**, RSS unchanged. The post-fix leaf profile is 100% the JS loop —
every runtime call is gone from the hot path.

No new env knob: the inline path rides `PERRY_STATIC_STRING_LOWERING`, the
gate the sibling inline `.length` fast path already uses. The fast path reads
`StringHeader` at offsets 0/4/20; because `perry-codegen` cannot depend on
`perry-runtime`, the struct definition gained a `const` assertion
(`STRING_HEADER_ABI_MATCHES_CODEGEN`) so a layout change fails the runtime
build instead of silently miscompiling every `.length` and `charCodeAt` in
every user program.

Two things deliberately NOT changed. `JSON.stringify` was listed in #7592 at
1,451 ms; re-measured it is **267 ms** — the old figure was GC pause charged to
the phase it landed in, and #7594/#7596 already removed it. And #7596's
`scavenge_nursery_cap_effective_bytes` gained a `max(influx_driven,
old_gen_reclaimable/2)` term with no test; the policy is now a pure function of
its two inputs and is covered directly, because the sibling cap-scale test only
*looks* like coverage — it asserts against the effective cap in a unit-test
thread whose old-gen is ~empty, so it stays green with the proportional term
deleted.
