# String operations lane October 10 2026

Baseline: `854fcd48ec295c258ea871024bc526c47ff2abc3`. This lane unifies construction and regex binding under the existing WTF-8 validity/count proof, removes repeated validation/counting of proven strings, and reuses the existing intern probe and hash authority. StringHeader remains 20 bytes; no ABI/version bump, new cache, side table, or latch.

## Attribution and implementation

Supplied evidence: `attribution-final-20261010.md` and the six raw `*.full.txt` reports under `attr-final-20261010/`. These are sampled **self cycles**, from another host and commit; they prioritize mechanisms rather than predict qb6 instruction deltas. Exact per-program string symbols are retained in `symbols.json`.

The largest entry points among the requested concat/slice/search/character/numeric operations, summed over the six reports, are `js_string_concat` (1.71 percentage points), `js_string_concat_chain` (1.26), and `js_string_concat_box` (0.91). Generic `js_string_coerce` contributes another 1.08 points and is also improved at its typed-output boundary. In qs stringify alone, those three concat entries are 1.03%, 0.90%, and 0.73%. The broader family totals in `attribution.json` also include shared `trunc`; its entire share cannot be attributed to string calls.

| Program | Prominent string operation symbols and self share | intern_dispatch / intern_lookup | from_utf8 |
| --- | --- | ---: | ---: |
| tsc | coerce 0.43%; index_to_i32 0.26%; last_index_of_from and char_code_at 0.13% each | 0.55% / 0.26% | 0.77% |
| qs parse | index_of_from, index_to_i32, concat 0.36% each | 0.31% / 0.88% | 1.24% |
| qs stringify | concat 1.03%; concat_chain 0.90%; concat_box 0.73% | 0.50% / unsampled | 0.50% |
| Commander | concat_chain 0.31%; concat_box and index_of_from 0.16% each | 1.25% / 0.78% | 0.94% |
| Effect | concat_value 0.09%; from_bytes_with_capacity 0.31% | 0.31% / 0.13% | 1.25% |
| Fastify | coerce 0.19%; pool_atom and from_bytes_with_capacity 0.14% each | 0.47% / 0.10% | 1.24% |

Operation labels abbreviate their `js_string_` prefix. Fastify also samples `atom_lookup` at 0.46%; that identity mechanism is unchanged. The from_utf8 column includes consumers outside these builtins.

- Construction establishes the existing `STRING_FLAG_WTF8_VALIDATED` proof from typed UTF-8/ASCII encoders or once at an unknown raw-byte boundary. The same proof certifies an exact UTF-16 count and the lone-surrogate flag. Checked borrows skip UTF-8 validation only for proven, surrogate-free payloads. Regex checked binding completes the same proof for legacy producers.
- Exact slices/copies preserve proof; concat and append intersect both encoding and JSON escaping proofs, union surrogate metadata, and preserve counted lengths. Boxed concat reads header metadata instead of recounting/revalidating operands. Pairwise concat opens roots only when the existing no-collection allocator cannot serve the request, then reloads source addresses after allocation. No GC/allocator policy changes.
- Canonical keys and atoms were inspected first. The intern cache already has one content probe per intern. Prehashed literal callers now use that same probe, with FNV folded at eligible literal call sites; descriptor-provided hashes remain reused. Rooted table entries avoid redundant pointer classification and an identical pointer avoids a byte compare. The header has no hash field, so none was added. Atom identity and intern eligibility remain distinct.
- String index coercion uses Rust's defined saturating/truncating cast, preserving ToNumber. One-byte search uses the character searcher; WTF-8 fallback searches UTF-16 units, including individual surrogate halves and overlapping lastIndexOf matches. Numeric concat uses the shared ECMAScript formatter; numeric/typed coercion output carries encoder proof directly.
- The only property-read edit replaces raw UTF-8 validation with the shared string borrow in `native_get::try_data_lookup_key`. No method-site/read-holder, prime-chain/key-add, global builtin-read, or explicit-this logic changes.

## Verification and measurement

Release builds and single-threaded runtime tests run only on qb6, under CPUs 0–55, Cargo `-j 8`. Immutable baseline and candidate have separate source directories and targets, including the compiler's coherent stream-dispatch targets. Node is pinned to 26.5.1. Instruction/RSS measurements use `taskset -c 0-55 setarch -R`, **n=5 interleaved** baseline/candidate/Node trials, off-lock; no cycles or wall-time acceptance claims. TSC has `PERRY_LL_RS4GC_MAX_INSTRS=2097152` in both arms. Full-collection diagnostics run separately from the measured trials.

Drivers: tsc transpileModule ×1; qs parse/stringify 20000 with warmup 1000; Commander 5000 with warmup 200; Fastify inject 500 with warmup 30; Zod 5000; Effect 2000 schema constructions plus 20000 decodes; hello; shared buffer-heavy and worker-heavy default workloads. Effect uses the supplied workload with a deterministic final count instead of timing output. Both heavy drivers pass on the pinned baseline, so both are included in the performance gate.

All 192 string, 185 JSON, 79 regex, 12 borrowed-key lookup and 7 numeric-coercion runtime tests pass. All 19 focused Node parity fixtures pass; after review fixes, only the affected fixtures and suites were rerun. The Unicode JSON length test now expects the existing encoding-proof bit as well as the unchanged bytes and UTF-16 length. Final real-program measurement tables are pending the TSC and heavy-driver batch.

Negative controls cover malformed/truncated/overlong raw input and every single-byte mutation of a mixed UTF-8/WTF-8 payload against the regex decoder, a proven operand combined with an unknown operand, legacy surrogate metadata certified by regex binding, deliberate intern hash collisions and separate atom identity, nonnumeric ToNumber inputs, multibyte search needles, surrogate halves, and overlapping search matches. A long parsed JSON token combined with quotes/control bytes fails Node parity under the intermediate union of JSON proof; the final intersection fixes that path. The token exceeds the direct-quoting threshold, so the control exercises the flag consumer. Perry has flat payloads, not ropes; the large-string control exceeds 1 MiB. The final provenance fixture also passes with forced moving GC: 530 copying minors, 23,255 moved objects, and 524 loop polls. The existing source-rooting control passes in both arms with 1,006 copying minors and 12 moved objects.

Required address-class and GC-root-holder lint failures are byte-for-byte identical on baseline and candidate after path normalization. File-size and Node-version checks pass. No baseline lint failures were edited in another lane's area.

The program-wide ≤2× Node target is not achieved: the completed qs, Commander, Fastify, Zod and Effect measurements already exceed it. This lane removes redundant string work; the supplied attribution also identifies larger property access, dispatch, allocation and GC costs. The prohibited read-path areas remain untouched. Unknown legacy string producers retain checked borrowing until they establish the existing proof; this lane does not claim that every runtime producer has been migrated.
