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

Drivers: tsc transpileModule ×1; qs parse/stringify 20000 with warmup 1000; Commander 5000 with warmup 200; Fastify inject 500 with warmup 30; Zod 5000; Effect 2000 schema constructions plus 20000 decodes; hello; shared buffer-heavy and worker-heavy default workloads. Dependencies are TypeScript 5.8.2, Zod 3.23.8, qs 6.16.0, Commander 15.0.0, Fastify 5.12.5 and Effect 4.0.0-beta.83. Effect uses the supplied workload with a deterministic final count instead of timing output. Both heavy drivers pass on the pinned baseline, so both are included in the performance gate.

All 192 string, 185 JSON, 79 regex, 12 borrowed-key lookup and 7 numeric-coercion runtime tests pass. All 19 focused Node parity fixtures pass; after review fixes, only the affected fixtures and suites were rerun. The Unicode JSON length test now expects the existing encoding-proof bit as well as the unchanged bytes and UTF-16 length. All ten real programs pass output/stderr/exit parity against Node in both arms and in the measured trials.

## Final instruction and RSS results

M denotes one million retired user instructions. Negative deltas favor the candidate. The noise estimate is `(max − min) / median` over the five same-binary trials, not a confidence interval. Worker scheduling has a much wider range than the other programs.

| Program | Main M instructions | Candidate M | Node M | Δ instructions | Candidate / Node | Same-binary range B / H |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| TSC | 8,287.054 | 8,240.214 | 3,783.547 | -0.565% | 2.178× | 0.0288% / 0.0335% |
| qs parse | 18,854.152 | 18,750.004 | 4,114.534 | -0.552% | 4.557× | 0.0015% / 0.0202% |
| qs stringify | 37,912.734 | 37,463.244 | 5,608.630 | -1.186% | 6.680× | 0.1369% / 0.1399% |
| Commander | 6,577.296 | 6,560.046 | 1,437.277 | -0.262% | 4.564× | 0.0006% / 0.0031% |
| Effect | 19,716.974 | 19,598.207 | 3,697.312 | -0.602% | 5.301× | 0.0073% / 0.0602% |
| Fastify | 4,134.471 | 4,106.130 | 1,512.838 | -0.685% | 2.714× | 0.3097% / 0.3301% |
| Zod ×5000 | 5,825.567 | 5,811.571 | 1,909.461 | -0.240% | 3.044× | 0.0071% / 0.0086% |
| hello | 1.362 | 1.361 | 327.060 | -0.026% | 0.004× | 0.0019% / 0.0035% |
| buffer_heavy | 10,139.129 | 10,120.334 | 8,112.801 | -0.185% | 1.247× | 0.0004% / 0.0004% |
| worker_heavy | 2,037.101 | 2,034.588 | 2,894.894 | -0.123% | 0.703× | 0.7371% / 1.0150% |

RSS is the peak reported by `/usr/bin/time`; GC counts come from separate diagnostic executions.

| Program | Main RSS KiB | Candidate RSS KiB | Node RSS KiB | Δ KiB | Full GC B / H |
| --- | ---: | ---: | ---: | ---: | ---: |
| TSC | 169,744 | 171,332 | 182,504 | +1,588 | 0 / 0 |
| qs parse | 36,468 | 36,296 | 85,184 | -172 | 0 / 0 |
| qs stringify | 37,796 | 38,324 | 88,788 | +528 | 0 / 0 |
| Commander | 35,856 | 34,576 | 86,720 | -1,280 | 0 / 0 |
| Effect | 141,116 | 144,136 | 210,132 | +3,020 | 1 / 1 |
| Fastify | 81,420 | 84,080 | 114,460 | +2,660 | 0 / 0 |
| Zod ×5000 | 36,452 | 37,300 | 81,688 | +848 | 0 / 0 |
| hello | 9,700 | 10,152 | 63,632 | +452 | 0 / 0 |
| buffer_heavy | 66,556 | 67,024 | 118,228 | +468 | 36 / 36 |
| worker_heavy | 110,988 | 110,684 | 407,568 | -304 | 42 / 40 |

The measured instruction reductions follow the removed work: qs stringify benefits most from counted concat operands, deferred root setup and shared number formatting; qs parse benefits from construction proof, checked-key/regex proof reuse, direct index conversion and one-byte search. Commander combines the literal intern probe changes with regex-proof reuse and search setup removal. Fastify, Zod and Effect benefit from shared proof consumption and intern/coercion setup. Hello removes a small amount of startup string construction work. TSC benefits from the same coerce/intern/proof changes and direct string index conversion. Buffer-heavy exercises template concatenation, number formatting and charCodeAt; its instruction reduction is stable with 36 full collections in both arms. Worker-heavy is flat within its 0.74–1.01% same-binary instruction range; its small median change is not a demonstrated speedup. These are aggregate mechanism explanations, not independent ablations of each edit.

THP-disabled n=5 controls preserve the Fastify (+2,664 KiB), Zod (+848 KiB) and Effect (+3,028 KiB) RSS increases. At the final stdout write, executable residency changes by +2,596, +832 and +2,828 KiB respectively, while anonymous RSS changes by +8, 0 and −32 KiB. With THP disabled, their anonymous deltas are 0, +4 and +4 KiB; AnonHugePages is zero. The inferred cause is executable code layout/page residency. ELF .text grows by only 112–160 KiB for these programs; changed placement spreads the executed functions across different resident pages. The full-collection counts remain 0/0, 0/0 and 1/1. No new retained runtime state is involved. The THP-off controls use process-local PR_SET_THP_DISABLE, not a host-wide setting. All measured snapshots confirm AnonHugePages=0 in that mode.

| Program | THP-off main RSS KiB | Candidate RSS KiB | Δ KiB | Full GC B / H |
| --- | ---: | ---: | ---: | ---: |
| TSC | 162,460 | 163,564 | +1,104 | 0 / 0 |
| qs parse | 27,264 | 27,644 | +380 | 0 / 0 |
| qs stringify | 29,216 | 29,212 | -4 | 0 / 0 |
| Commander | 26,496 | 25,216 | -1,280 | 0 / 0 |
| Effect | 132,404 | 135,432 | +3,028 | 1 / 1 |
| Fastify | 71,944 | 74,608 | +2,664 | 0 / 0 |
| Zod ×5000 | 26,964 | 27,812 | +848 | 0 / 0 |
| hello | 7,340 | 7,792 | +452 | 0 / 0 |
| buffer_heavy | 61,112 | 61,560 | +448 | 36 / 36 |
| worker_heavy | 90,184 | 89,856 | -328 | 42 / 41 |


At stdout, normal-mode anonymous RSS is equal in both arms for hello, qs parse, Commander, Zod and TSC; qs stringify differs by +8 KiB, Fastify by +8 KiB, Effect by −32 KiB, and buffer-heavy by −8 KiB. Normal-mode AnonHugePages is identical in each of those A/B pairs. Executable residency differences are +212 KiB (hello), +68 (qs parse), +304 (qs stringify), −1,392 (Commander), +2,596 (Fastify), +832 (Zod), +2,828 (Effect), +1,552 (TSC), and +864 (buffer-heavy). ELF .text grows by 107–160 KiB across the ten binaries. Peak RSS and final resident pages are different phases; the snapshot evidence attributes the stable changes to image placement/page residency rather than added string-header storage.

The smaller RSS deltas follow that same mechanism. Hello's +452 KiB peak includes startup/teardown image residency. qs parse changes sign in the THP-off control (−172 to +380 KiB) while final anonymous RSS is equal. qs stringify's +528 KiB normal peak becomes −4 KiB with THP off; baseline THP-off trials themselves span 528 KiB. Commander retains its −1,280 KiB peak difference with THP off, consistent with fewer executable pages touched. TSC's +1,588 KiB peak becomes +1,104 KiB with THP off; normal anonymous RSS and 53,248 KiB of huge pages are identical. Its THP-off anonymous snapshot is +364 KiB, below 0.5% of that anonymous mapping. Buffer-heavy's +468 KiB is inside the candidate's 536 KiB same-binary RSS range; final anonymous RSS is unchanged to within 8 KiB and GC counts stay 36/36.

Worker-heavy's RSS spans 105,100–120,500 KiB on main and 103,548–113,152 KiB on the candidate, so the −304 KiB median is noise. Its job assignment depends on message arrival: the same 400 jobs can be distributed differently over four worker heaps, changing concurrent live buffers and collection counts. Five additional interleaved diagnostic controls give main full counts [41,42,42,41,41] and candidate [42,41,41,41,41], both medians 41. Including the original normal and THP-off diagnostics, candidate counts span 40–42; this is scheduling variation, not a GC policy change.

Negative controls cover malformed/truncated/overlong raw input and every single-byte mutation of a mixed UTF-8/WTF-8 payload against the regex decoder, a proven operand combined with an unknown operand, legacy surrogate metadata certified by regex binding, deliberate intern hash collisions and separate atom identity, nonnumeric ToNumber inputs, multibyte search needles, surrogate halves, and overlapping search matches. A long parsed JSON token combined with quotes/control bytes fails Node parity under the intermediate union of JSON proof; the final intersection fixes that path. The token exceeds the direct-quoting threshold, so the control exercises the flag consumer. Perry has flat payloads, not ropes; the large-string control exceeds 1 MiB. The final provenance fixture also passes with forced moving GC: 530 copying minors, 23,255 moved objects, and 524 loop polls. The existing source-rooting control passes in both arms with 1,006 copying minors and 12 moved objects.

Required address-class and GC-root-holder lint failures are byte-for-byte identical on baseline and candidate after path normalization. File-size and Node-version checks pass. No baseline lint failures were edited in another lane's area.

The program-wide ≤2× Node target remains unfinished. TSC, qs, Commander, Fastify, Zod and Effect instruction ratios exceed 2×; no wall-time gate was run. This lane removes redundant string work; the supplied attribution also identifies larger property access, dispatch, allocation and GC costs. The prohibited read-path areas remain untouched. Unknown legacy string producers retain checked borrowing until they establish the existing proof; this lane does not claim that every runtime producer has been migrated.

## Artifacts and limits

Raw n=5 counters, RSS files, GC diagnostics, parity outputs, negative-control failures, residency snapshots, drivers and binary SHA-256/ELF-section manifests are retained in `/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/strops-evidence/`. `verify/results.json` and `verify/thp-results.json` hold final normal and process-local THP-off results for all ten programs. Runtime/driver build targets are removed after verification; independently linked program executables remain in this lane's qb6 hostdir.

Build: `cargo build --release -j 8 -p perry -p perry-runtime -p perry-runtime-static -p perry-stdlib-static`; Rust 1.101.0-nightly (db8f076d2, 2026-10-03), LLVM 21, Node 26.5.1, perf 7.0.14. Candidate production source is `79849d54e7`; `9d9f12107f` subsequently adjusts only a cfg(test) assertion. Later commits record evidence. No versions were changed. All commits use Ralph Küpper <ralph3@skelpo.com> as author and committer.

Bundle: `/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/strops.bundle`, with prerequisite `854fcd48ec295c258ea871024bc526c47ff2abc3`. This report is also copied to `scratchpad/codex-small/strops-report.md`.
