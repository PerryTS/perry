Accessor identity and data attributes now belong to the holder shape for every object, including arrays and built-in prototypes. Arrays use their existing traced named-property reserve; functions, byte owners, exotic cells and native handles use their existing property storage. Installing or replacing a descriptor changes the holder shape and invalidates receiver-to-holder memos.

Removed the global owner-address/key descriptor maps, accessor_keys_by_owner, descriptor young logs, root scanners, move/rekey and dead-owner bookkeeping. Removed the process-wide accessor/constructor gates, declared-field-name/prototype-key sets, PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED, their codegen loads and the N1 per-literal fallback. Former descriptor-summary header words are reserved ABI padding. No descriptor cache, owner index, latch or name-vetting mechanism was added.

The named fix-forward, direct holder-slot attribute reads, is implemented in commit b046f71886. Method/read-holder admission carries the existing slot and ShapeId proof, reads the attribute byte from the validated holder shape and avoids a second receiver classification or name lookup. Field-cache, wide-index and ordinary resolved-slot reads also carry their known slot and live bound directly. Accessor-pair lookup carries one shape record through its single initial property-name lookup. Memo layout is unchanged. Getter calls retain their receiver and root lifetime rules; keys and raw holder pointers are not reused after a collecting getter.

Validation is against origin/main 6521c0cbf46b684b73cf0a35445ae3e39310e387, after clean rebases incorporating the upstream GC/RegExp and iterator changes. merge-tree returned 0 and changed files have no conflict markers. Production source IDs are main src:a6831a364daefd5f6ece73151f967d0e0426df096f5812fd14498466c4131369 and head src:d3bc796adf3f0d6869ac7bca2047dc0b2c1f04a5bf21982cd6322f9d7355c64d. Separate four-package release builds pass. Linux builds/tests use CPUs 0–55, at most eight Cargo jobs and one test thread. No versions were changed in this lane. After the final measurements, five PayloadBuffer/zlib commits advanced the shared origin/main reference to ac8b70fe5559e7bd92b602d2f2dc912a6c9b0b47. merge-tree against that head returns 0; those late upstream changes were not part of this A/B or test population. The delivered bundle retains the fully tested 6521c0cbf4 prerequisite.

| Check | Main | Head | New failures |
| --- | ---: | ---: | ---: |
| Runtime unit tests | 5,276 pass, 5 ignored | 5,267 pass, 5 ignored | 0 |
| Codegen unit tests | 2,068 pass, 1 ignored | 2,068 pass, 1 ignored | 0 |
| Codegen integration tests | All pass | All pass | 0 |
| Stdlib unit tests | 250 pass, 2 fail | 250 pass, same 2 fail | 0 |
| Selected gap tests | 233/238 pass | 235/238 pass | 0 |
| Method-site integration | 14 pass, 1 fail | 14 pass, same 1 fail | 0 |

The two stdlib failures are thread_exit_releases_the_threads_closure_side_table_entries and symbols_tests::thread_exit_releases_the_threads_symbol_side_table_entries. The method-site failure is function_object_receivers_are_served_from_their_property_object, with the same zero counters in both arms. Shared gap failures are test_gap_iterret_generator_prototype, test_gap_iterator_next_foreign_prototype and test_gap_iterator_step_generator_prototypes; the latter two came from the latest upstream iterator commits. The two new #12015 fixtures fail against Node on main and pass here. test_gap_11910_member_call_order passes in both arms. Nine deleted descriptor-table lifecycle tests explain the runtime population difference.

Fixtures and runtime tests cover late accessor installation after memo warming, literal writes and reads through Object.prototype/Array.prototype, array/built-in descriptor reflection, array growth and freezing, copying-GC relocation of holders/getters, native allocations with misleading header bits, and accessor/data replacement in narrow, wide and dictionary holders.

N1 medians are instructions per literal, n=5 interleaved. Native slopes use 20,000/80,000 iterations; Node 26.5.1 uses 200,000/1,600,000. Fixed startup cost is subtracted. m_lit_fo forces PERRY_FULL_OUTLINE_IC=1. Raw Node slopes retain JIT variation.

| Micro | Accessor | Main | Head | Node 26.5.1 |
| --- | --- | ---: | ---: | ---: |
| m_lit | absent | 385.3 | 377.3 | 183.7 |
| m_lit | installed | 498.2 | 377.2 | 189.4 |
| m_lit_fo | absent | 428.3 | 421.3 | 183.6 |
| m_lit_fo | installed | 7085.7 | 421.2 | 184.2 |

The poisoned full-outline case removes 94.1% of instructions. The absent-accessor full-outline case saves seven instructions per literal; the installed accessor no longer sends every literal through constructor/four guarded-store continuations.

In a disposable source copy and separate target, discarding descriptor shape facts makes the warmed-holder test fail at its unchanged-shape assertion and the resolved-slot test return NaN instead of 41. Public prototype/descriptor reflection diverges from Node; the callback-order fixture throws TypeError: push is not a function. Restoring and arming the process-wide constructor/store gates changes full-outline cost from 428.3 without the accessor to 3,474.7 with it; the unchanged control is 421.3/421.2. Thus semantic and cost assertions turn red. No sabotage is in production.

All ten real programs and both literal witnesses match Node on both arms: TypeScript (3), Zod (5,000), qs parse/stringify (20,000 × 200), commander (5,000 × 200), hello, Fastify inject (2,000 × 50), Effect 4.0.0-beta.83, buffer-heavy and worker-heavy. The last two pass current main and are gated here. Medians are five interleaved runs under /root/MEASURE.lock, CPUs 56–63, disabled ASLR. Binaries are stripped; separate symbol copies retain identical code/build IDs. Fresh instruction profiles cover both arms of all ten programs, with 64 MiB perf buffers and no lost-buffer warnings.

| Program | Main instructions:u | Head instructions:u | Change | RSS KiB main → head | Full starts main / head | Instruction span % main / head |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| tsc | 26,411,414,372 | 26,100,995,847 | -1.175% | 231,784 → 235,452 | 1 / 1 | 0.0168 / 0.0226 |
| zod | 13,660,879,971 | 13,704,704,778 | +0.321% | 36,564 → 35,992 | 0 / 0 | 0.0248 / 0.0574 |
| qsparse | 21,417,701,214 | 21,370,758,965 | -0.219% | 36,728 → 36,828 | 0 / 0 | 0.0006 / 0.0008 |
| qsstr | 50,781,540,834 | 50,686,641,851 | -0.187% | 38,548 → 38,276 | 0 / 0 | 0.0006 / 0.0004 |
| commander | 7,231,484,460 | 7,221,626,869 | -0.136% | 34,488 → 34,516 | 0 / 0 | 0.0004 / 0.0010 |
| hello | 1,375,483 | 1,373,451 | -0.148% | 10,224 → 10,156 | 0 / 0 | 0.0023 / 0.0008 |
| fastify | 24,568,568,716 | 24,555,128,790 | -0.055% | 151,908 → 152,088 | 0 / 0 | 0.1096 / 0.2301 |
| effect | 22,448,151,434 | 22,419,710,706 | -0.127% | 146,780 → 145,860 | 1 / 1 | 0.1435 / 0.0065 |
| buffer_heavy | 11,256,400,015 | 11,250,000,211 | -0.057% | 80,904 → 79,820 | 36 / 36 | 0.0052 / 0.0040 |
| worker_heavy | 2,113,422,726 | 2,128,840,445 | +0.730% | 134,108 → 130,224 | 41 / 41 | 1.7993 / 0.9815 |

Full starts sum synchronous [gc-full] and [gc-budgeted] start kind=full lines across threads. Counts are medians of five separate diagnostic runs; diagnostics are disabled for instruction/RSS measurements. The last column records the empirical same-binary min–max instruction span.

Zod's +0.321% is the only positive instruction delta outside measured noise, below 0.5%. Its named cost is ordinary-holder admission and shape/key-attribute reads replacing owner/global descriptor summaries. The profile shows own_accessor, object_key_entry_filtered, object_keys_and_live_slot_count and descriptor_route; their percentages are attribution evidence, not exact instruction budgets. Direct holder-slot reads remove redundant classification and lookup from known-slot paths.

TypeScript, qs, commander and buffer-heavy gain from deleting constructor/field guards and repeated accessor/key queries. TypeScript's get_accessor_descriptor profile share falls from 0.73% to 0.08%, key-entry queries from 0.51% to 0.39%; executable text shrinks 651,106 bytes (636 KiB). Commander reduces guarded class-field work; stream/byte-owner paths benefit from resolved-slot reads. Hello saves 2,032 fixed startup instructions after descriptor-state setup/teardown deletion. Fastify and Effect changes fall inside measured noise; Effect's remaining holder-query cost is balanced by removing repeated slot admission and guards.

Worker-heavy retains its standard four-worker row above. All controls use n=5 with the same 400 jobs and Node checksum:

| Worker control | Instructions change | RSS KiB main → head | Full starts | Instruction span % main / head |
| --- | ---: | ---: | ---: | ---: |
| Standard primary | +0.730% | 134,108 → 130,224 | 41 / 41 | 1.7993 / 0.9815 |
| Standard repeat | +1.506% | 129,980 → 131,232 | 42 / 42 | 1.9417 / 1.4330 |
| THP disabled | -0.566% | 111,996 → 111,872 | 41 / 42 | 1.2786 / 2.6851 |
| One worker diagnostic | +0.262% | 68,104 → 67,304 | 48 / 48 | 0.0414 / 0.1425 |

The concurrent changes are within the recorded noise envelopes, vary with worker allocation distribution and GC trace/sweep work, and reverse sign with THP disabled. The fixed-pool diagnostic isolates a bounded holder/key-query contribution below 0.5%; it does not replace the four-worker row. Worker profiles identify ValidPointerSetBuilder, tracing, reclamation and finalization as substantial work. Primary/repeat RSS spans reach 6–9 MiB and reverse sign. No allocator or pacing policy changed. The old pre-fix-forward +0.8–1.0% comparison is superseded, not declared accepted.

TypeScript RSS is +3,668 KiB normally and +4,472 KiB with THP disabled (226,520 → 230,992 KiB). Both arms have one full collection and four minors; sampled AnonHugePages is zero in every disabled arm. This increase is explained by allocation progress during incremental collection and existing nursery block commitment. In retained diagnostics, full completion occurs at 152,863,200 → 155,078,408 allocated bytes (+2,215,208), with 28,193,824 → 30,313,704 live bytes (+2,119,880), but mature live bytes increase only 23,152. Both complete in 65 steps. Allocate-black births remain live until a subsequent collection; changing guarded stores and descriptor root ownership shifts that completion boundary without changing collector policy.

Three interleaved diagnostic pairs append gc() after the unchanged TypeScript workload. After that explicit collection, reachable live bytes are 4,693,976 → 4,693,088 (−888), live objects 58,651 → 58,621, and side metadata is approximately 42 KiB smaller. Eden capacity is 30 → 31 existing 2 MiB blocks while used Eden bytes fall 57,295,304 → 55,199,392. These results support transient allocate-black occupancy and block commitment as the mechanism; the connection from changed field/root work to the shifted completion boundary is an inference from the code and counters. The diagnostic copies are separate from every production measurement, preserve Node stdout and are not substituted into the RSS table. The signal-only attempt was not serviced by the synchronous driver; explicit-GC census runs and their confirmation on test CPUs are retained.

Other RSS changes are below a block or within observed run spans: Effect has 3.5 MiB spans, worker primary/repeat has 6–9 MiB spans, and the worker THP-off change is −124 KiB. Buffer-heavy’s −1,084 KiB occurs with the same 36 full collections. Smaller generated guard/GC metadata and property-query allocation/placement account for fixed-footprint reductions; page-placement attribution is an inference, not an exact byte budget. The same allocator and pacing policy apply in both arms.

The store audit has the same 19 findings. Existing file-size violations remain delete_rest.rs (2,005 → 2,002 lines) and method_site.rs (2,235 → 2,184). Node-version consistency passes. Root, native-handle, registry-lifetime and thread-exit inventory diagnostics match main; two deleted flags reduce classified address-global candidates by two. The existing PASS1_MARKED census annotation's gc/mod.rs hash/rationale was refreshed after deleting descriptor scanner/startup hooks. Its full-mark publication, TLS consumption before census and no-yield lifetime were reviewed; no new classification or exemption was added.

Full raw results, source provenance, five slopes/runs, diagnostic logs, symbol profiles and sabotage proofs are retained under /Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/meta3/fix-forward-latest/. Earlier fix-forward and fix-forward-current rounds are retained as superseded evidence.
