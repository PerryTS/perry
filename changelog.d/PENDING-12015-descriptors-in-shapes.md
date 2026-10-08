Accessor identity and data attributes now belong to the holder shape for arrays and built-in prototypes, using the same key entries and accessor-pair slots as ordinary objects. Arrays reach their holder through the existing traced named-property reserve; functions, byte owners, exotic cells and native handles use their existing property storage. Descriptor installation changes the holder shape, retiring receiver-to-holder memos. Method-call preparation reads that shape fact directly.

Removed the global address/key descriptor maps, their owner indexes (including `accessor_keys_by_owner`), descriptor young logs, root scanning, move/rekey and dead-owner bookkeeping. Removed the process-wide descriptor/accessor gates, declared-field-name/prototype-key hash sets, constructor-accessor latch and `PERRY_CLASS_FIELD_INLINE_GUARD_DISABLED`, including the codegen loads and N1 full-outline fallback gate. Metadata words previously used for descriptor summaries are reserved ABI padding; they are no longer descriptor authorities. No descriptor cache, side table, latch or name-vetting mechanism was added.

Descriptor installation and replacement retire the holder shape, so warmed receiver-to-holder memos cannot keep serving the previous data slot or getter. `js_method_site_prepare` reads accessor identity from those shape entries. Fresh literal stores use their own data-slot proof and no process-wide descriptor state. Native-handle recycling releases its existing holder edge instead of treating a native allocation as an object header.

Validation is against origin/main `211016c69849022913e071a8ae0a26656d8c3677`, with the implementation rebased onto it. Both four-package release builds pass. The two arms use separate Linux targets, CPU affinity 0–55, at most eight Cargo jobs, and `--test-threads=1`.

| Check | Main | Descriptor shapes | New failures |
| --- | ---: | ---: | ---: |
| Runtime unit tests | 5,268 pass, 5 ignored | 5,258 pass, 5 ignored | 0 |
| Codegen unit tests | 2,067 pass, 1 ignored | 2,067 pass, 1 ignored | 0 |
| Codegen integration tests | All pass | All pass | 0 |
| Stdlib unit tests | 250 pass, 2 fail | 250 pass, same 2 fail | 0 |
| Existing selected gap tests | 233/234 pass | 233/234 pass | 0 |
| New descriptor gap fixtures | Both fail against Node | Both match Node | 0 |
| Additional method-site integration | 14 pass, 1 fail | 14 pass, same 1 fail | 0 |

The two stdlib failures are the existing closure and symbol thread-exit assertions. Both gap runs retain only `test_gap_iterret_generator_prototype`; the candidate passes 235/236 including the two new descriptor fixtures. The fixtures were also compiled on this main in a separate two-case run and both fail against Node. `test_gap_11910_member_call_order.ts` passes in both arms. The additional method-site suite has 14 passes and the same `function_object_receivers_are_served_from_their_property_object` counter assertion failure in both arms (own=0, inherited=0, function=0). Removed descriptor-table lifecycle tests account for the smaller runtime test population.

The new fixtures cover warmed reads, Object.prototype and Array.prototype getter/setter semantics, own-property shadowing, array indices and named properties through growth, descriptor reflection on arrays and built-in prototypes, exotic accessor deletion, 43 array callback-order cases, symbols, and rebinding globalThis.Array. Runtime tests also cover real copying-GC relocation of holders and getter closures, freezing a full array while its reserve grows, and native allocations with misleading GC-like headers.

**N1: median instructions per literal, n=5 interleaved.** `m_lit_fo` forces `PERRY_FULL_OUTLINE_IC=1`; `desc` installs the unrelated class-prototype accessor before the loop. Native slopes use 20,000 and 80,000 iterations; Node slopes use 200,000 and 1,600,000. Fixed startup cost is subtracted. Node 26.5.1 has JIT variation, recorded in the five raw slopes.

| Micro | Accessor | Main | Descriptor shapes | Node |
| --- | --- | ---: | ---: | ---: |
| m_lit | absent | 385.3 | 377.3 | 183.0 |
| m_lit | installed | 498.2 | 377.2 | 186.9 |
| m_lit_fo | absent | 428.3 | 421.3 | 189.8 |
| m_lit_fo | installed | 7,085.7 | 421.2 | 188.3 |

The full-outline accessor case removes 94.1% of instructions. Removing the volatile constructor gate saves seven instructions per literal without the accessor; with it, the literal keeps its direct stores instead of calling the constructor and four guarded field-store continuations.

**Sabotage proofs.** In a disposable source copy and separate target, discarding data/accessor shape facts makes the warmed-holder test fail at the unchanged-shape assertion. The prototype reads and setter stores diverge from Node, and array/built-in descriptor reflection reports wrong attributes. The array callback-order fixture compiles but throws `TypeError: push is not a function` when the sabotaged accessor is exposed as data. Restoring the constructor and runtime gate loads, armed by a public accessor install, changes full-outline literal cost from 428.3 without the accessor to 3,474.7 with it; the unchanged control stays at 421.3/421.2. This turns the N1 flat-cost assertion red while stdout remains identical. No sabotage is present in the production tree.

**Real programs.** TypeScript (3), Zod (5,000), qs parse and stringify (20,000 × 200), commander (5,000 × 200), hello, Fastify inject (2,000 × 50), Effect 4.0.0-beta.83, buffer-heavy and worker-heavy all match Node in both arms. Buffer-heavy and worker-heavy now pass main and are included. Medians use five interleaved runs under `/root/MEASURE.lock`, CPUs 56–63 and disabled ASLR. Measurement binaries are stripped; their separate symbol copies retain identical code and build IDs.

| Program | Main instructions:u | Shape instructions:u | Change | RSS KiB main → shapes | Full starts main / shapes | Instruction span % main / shapes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| tsc | 27,074,035,007 | 26,861,598,782 | -0.785% | 222,008 → 219,780 | 1 / 1 | 0.0248 / 0.0427 |
| zod | 14,708,511,183 | 14,757,771,754 | +0.335% | 36,532 → 36,556 | 0 / 0 | 0.0374 / 0.0487 |
| qsparse | 22,088,233,469 | 22,091,575,818 | +0.015% | 36,784 → 36,808 | 0 / 0 | 0.0009 / 0.0009 |
| qsstr | 51,918,886,323 | 51,995,641,239 | +0.148% | 38,088 → 38,728 | 0 / 0 | 0.0015 / 0.0960 |
| commander | 7,656,092,747 | 7,654,344,651 | -0.023% | 34,728 → 34,756 | 0 / 0 | 0.0010 / 0.0052 |
| hello | 1,380,426 | 1,379,142 | -0.093% | 9,664 → 9,724 | 0 / 0 | 0.0020 / 0.0022 |
| fastify | 30,008,984,130 | 30,158,919,570 | +0.500% | 159,552 → 159,212 | 0 / 0 | 0.0629 / 0.1597 |
| effect | 22,784,034,365 | 22,855,880,413 | +0.315% | 146,280 → 147,956 | 1 / 1 | 0.0581 / 0.0130 |
| buffer_heavy | 11,299,540,345 | 11,305,461,355 | +0.052% | 80,916 → 79,820 | 36 / 36 | 0.0020 / 0.0076 |
| worker_heavy | 2,150,087,114 | 2,167,508,261 | +0.810% | 127,964 → 132,928 | 41 / 41 | 0.5515 / 0.2929 |

Full starts count synchronous `[gc-full]` lines plus `[gc-budgeted] start kind=full`, summed across threads. The counts are medians of five separate diagnostic runs per arm; diagnostic logging is disabled for instruction/RSS measurements. The empirical noise envelope is the same-binary min–max instruction span in the last column. An initial parser incorrectly expected JSON; all retained diagnostic logs were re-counted with the text format, and the corrected summaries supersede the earlier zero counts.

The remaining positive deltas come from holder admission, shape-record resolution and key-attribute reads in property paths that previously consulted owner summaries or global descriptor state. Fastify, Zod, Effect and qs profiles identify `descriptor_route`, `object_key_entry_filtered`, `object_keys_and_live_slot_count` and `object_key_blocks_plain_store`. Ordinary holders are admitted before closure/exotic classification; stores use the existing accessor filter when all data properties are writable; attribute queries carry their already-resolved shape record into the key lookup. These remove repeated classification, unrelated data-attribute probes and a second shape-record lookup without adding state.

TypeScript gains from deleting the constructor gate and class-field descriptor/prototype walks. Commander balances those savings against holder/key queries: its profile reduces class-field add/guard and `get_accessor_descriptor` work. Hello has only startup and one console call; its fixed instruction saving comes from deleting descriptor-state setup and teardown. Buffer-heavy adds bounded holder/key-query work in stream and byte-owner property paths, with the same 36 full collections and two copying minors. The profiles are attribution evidence; sample percentages are not exact instruction budgets.

Worker-heavy uses four concurrent workers. Its standard batch is +0.810%, the second five-run batch +0.551%, and the THP-off batch +1.035%. Same-binary instruction spans reach 2.0%; diagnostic full counts vary between 41 and 42, and RSS differences reverse sign with 8–10 MiB run spans. A diagnostic with the same 400 jobs and one worker still matches Node: +0.343% instructions, spans 0.0165% / 0.0354%, and 48 / 48 full collections. This isolates a small holder-query contribution with a fixed worker pool; the concurrent rows also vary with allocation distribution and collection work. The four-worker medians exceed the nominal allowance in some batches, so that performance result is reported for owner review and is not declared accepted. The standard four-worker row is retained above rather than replaced with the diagnostic.

RSS controls: with THP disabled per process, TypeScript is 215,744 → 213,724 KiB (−2,020), Effect 139,112 → 139,980 (+868), and worker-heavy 109,816 → 109,364 (−452). Full starts are 1 / 1, 1 / 1 and 41 / 41. Sampled AnonHugePages is zero in every disabled arm; normal TypeScript is 102,400 → 100,352 KiB. The TypeScript reduction survives without huge pages; attribution to the smaller generated guarded continuations/GC metadata and descriptor bookkeeping is an inference from the code changes and profiles. Effect and worker RSS also depend on nursery/allocator page placement; no allocator or pacing policy was changed.

**Named fix-forward: #12015 direct holder-slot attribute reads.** Implemented by using the resolved slot in method/read-holder admission: its current holder shape supplies the key-entry byte, and the existing memo retains the same slot and ShapeId proof. Generic field-cache, wide-index and ordinary key-lookup reads also use their validated slot and inline bound directly, including builtin accessors. Accessor-pair lookup carries one resolved shape record through its single name lookup and pair read. Shape changes continue to invalidate the memos. No memo layout, cache, owner index, latch or name check was added. The tables above are the pre-fix-forward comparison; the refreshed-main validation and measurements will replace them when complete.

The store audit has the same 19 findings in both arms. The two file-size violations already exist on main and both files are smaller here. Owner/root inventory failures are unchanged. The Node-version lint passes after the upstream CI corrections. No exemption was added to hide a new failure.
