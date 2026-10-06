Route native byte consumers through scoped byte access or owner-thread pins. Crypto, WebCrypto, SQLite, TLS, HTTP/2, ethers, web streams/BYOB, StringDecoder, querystring, filesystem results, TextEncoder/TextDecoder, V8/VM and structured-clone copies no longer derive byte addresses from buffer headers. `copy_value` roots a source before destination allocation and resolves its bytes afterward.

Compatible B4 changes move detached state into the owner header (deleting `DETACHED_BUFFER_REGISTRY` and `EVER_DETACHED`), replace `INLINE_OWNING_U32_CACHE` with a current-header admission check, and prefix all three persistent-symbol factories with an eight-byte leaf GC header. Detached state uses bit 14, leaving bits 3–5 for GC survival age; the existing bounded pin counter uses bits 9–13 (31 simultaneous pins per owner, returning `PinLimit` on overflow). The source gate also checks old raw byte-helper calls and emitted offsets; `run_lint_gates.sh` discovers it through the existing lint workflow. Child-process sabotages exercise the detector and the runtime/consumer contracts.

Boundary still pending: generated code directly links `PERRY_U8_INLINE_CACHE` and `PERRY_TA_KIND_CACHE`, and reads typed-array metadata at +8/+10. The proposed unified layout puts a data/owner pointer at +8. Removing those exported caches or replacing those fields requires the B4c codegen switch, explicitly excluded from this lane until #12023. The six-table deletion and unified 16-byte layout therefore cannot all be completed within that boundary. No placement policy, versions, fetch bodies, zlib implementation, or runtime node_stream files are changed.

The remaining B2c work includes native-addon APIs that return raw pointers, private Array.sort byte access, and typed-array creation paths coupled to the old layout. Their pointer lifetimes or representation must be adapted before the source gate can become a zero-debt invariant. This delivery is the compatible conversion subset, not completion of all B2c/B4 requirements. Zlib retains B1's wrapper and output witness.

The exact closed census rows are in `scripts/buffer_b4_census_closed.tsv`: 73 rows (42 runtime, 27 stdlib, 2 updater, 1 ext-http, 1 ext-net), comprising 35 creation, 28 size-assumption, 8 unscoped-borrow and 2 pointer-across-GC sites. Other ext producers already use B1's C ABI wrapper. The expanded source gate has 284 existing sites and zero additions; the same detector found 444 at the earlier B1 baseline, with 161 removed by this lane, one by upstream #12094, and two subsequent upstream verifier/test reads added. It is a ratchet, not the still-pending zero-debt layout invariant.

| Machinery | Deleted | Still pending |
|---|---|---|
| Address-keyed tables | `DETACHED_BUFFER_REGISTRY` | `VIEW_REGISTRY`, `BACKING_TO_VIEWS`, `RESIZABLE_BUFFER_MAX`, `BUFFER_AB_ALIAS`, `TYPED_ARRAY_VIEW_META` |
| Latches | `EVER_DETACHED` | `RESIZABLE_BUFFER_EVER_MARKED`, `BUFFER_AB_ALIAS_EVER_SET` |
| Address caches | `INLINE_OWNING_U32_CACHE` | `PERRY_U8_INLINE_CACHE`, `PERRY_TA_KIND_CACHE` |

| Contract | Witness and sabotage | Result |
|---|---|---|
| T1/T2: moving collection; borrow allocation forbidden | B1 byte tests; native-store pin remains rooted while a young object actually moves; allocation inside no_gc aborts | Native contract PASS / RED; the forced young-byte B6-knob variant is not run |
| T3: detach during operation | Deflate data listener transfers the input owner; native last-pin lifetime witness with B1 `detach_free` and B4 `detach_mark` sabotages | Node parity PASS on main and head; Native ASan witness PASS / RED; the TS stream itself is not run under ASan |
| T4: worker transfer | 32 MiB transfer preserves backing pointer, sender length zero, receiver contents and backing count; `transfer_copy` | PASS / RED |
| T5: large concat and nested views | 300 × 18,000 bytes; only the nested view roots its owner across minor and full GC; `view_edge` | Content/identity/owner lifetime PASS / RED; the edge still uses the legacy registry; the placement-counter variant awaits B3 |
| T6: owner access checks | u8, i32 and DataView share writes; shrink/OOB/grow/detach; `owner_check` | PASS / RED |
| T7: native outputs | Crypto sizes 0, 1, 255, 256, 257 and 1 MiB; random fill bounds; `crypto_output`, `crypto_borrow`, `random_fill_range`; B1 C ABI producer contracts | Consumer contracts PASS / RED; the physical-placement test with INLINE_MAX forced to 256 awaits B3/B4 |
| T8: Buffer parameter view | Main's `test_gap_12094_buffer_param_views` | Main and head PASS |
| T9: whole-module invariant | Source gate; owner-edge witness; header and root-holder gates | Ratchet PASS, nine source sabotages RED; address-table-free invariant pending B4c |
| Symbol header and u32 admission | Three persistent-symbol factories; current-header u32 admission; `symbol_header`, `u32_admission` | PASS / RED |

Verification baseline: origin/main `81e65f33ebdc72d75431b03427dcbfb2f153d7b1`; tested B1 `f3e59107868b737b3b5646a7d0b19dc4f9193f8f`. The final required refresh rebases onto B1 `16b16c96f3cb1f4f81a4bee62dc78c94be59cf83`; its additions are documentation and cfg(test) only, with no production change. The seven FFI buffer tests, including the added native-input consumer and its RED sabotage, pass with `runtime-link`. The final release builds, all 54 crate-test binaries, gap comparison, output checks and measurements include current main's iterator-close and from-space verifier changes, plus B1's process-shared pin fix. Measured production sources are those of `48205175a7`; later changes are harnesses, evidence and cfg(test) witnesses.

| Crate | Main passed / failed / ignored | Head passed / failed / ignored | New failures |
|---|---|---|---|
| runtime | 5,156 / 0 / 5 | 5,169 / 0 / 5 | 0 |
| stdlib | 251 / 2 / 0 | 258 / 2 / 0 | 0 |
| codegen | 2,564 / 0 / 1 | 2,564 / 0 / 1 | 0 |

The identical stdlib failures are `runtime_thread_exit_tests::symbols_tests::thread_exit_releases_the_threads_symbol_side_table_entries` and `runtime_thread_exit_tests::thread_exit_releases_the_threads_closure_side_table_entries`. The requested gap union contains 173 cases. Raw main results are 158 pass / 15 parity failures; raw head results are 157 / 16. The single difference is `test_gap_webcrypto_async_threadpool`: head's Node oracle printed `subtle.digest crosses macrotask: false`, while Perry printed true. Ten isolated, interleaved Node comparisons (five per arm) all passed. The original discrepancy is retained as Node timing variation, with zero persistent regressions established. The remaining 15 parity failures reproduce on both arms. Both #12094 and the new detach-during-zlib fixture pass.

All 14 program/kernel output checks pass against Node. Forced-GC focused tests pass: five B4, eight B1 byte-access, one transfer and three crypto tests, with six B4, seven B1 and three crypto child sabotages red. ASan's instrumented runtime with the system allocator passes 14 native tests (eight byte-access, five B4, one transfer), including the runtime child sabotages; leak detection is disabled for intentional process-lifetime objects. The TS zlib stream's output is checked against Node; that stream executable is not run under ASan.

All 26 distinct byte-contract/source sabotages turn red (16 runtime/crypto, nine source-gate, one FFI). Final source-layout, header-constant, root-holder and pin-custody checks pass, as do their applicable self-tests and the lint-runner inventory self-test. The full lint tier is not claimed: the file-size gate has three unchanged main violations (`dynamic_dispatch.rs`, `delete_rest.rs`, `method_site.rs`). Exact crate summaries, raw gap outcomes, rechecks, ASan summaries and output statuses are recorded in `scripts/fixtures/buffer_b4_verification.json`; interleaved trials and medians are in `scripts/fixtures/buffer_b4_measurements.json`.

Measurements use qb6 CPUs 56–63 under the shared lock, ASLR disabled, separate targets, Node 26.5.1 output checks and n=5 interleaved trials with identical-main-binary controls. Both arms use the same full prebuilt archives and forced http/net/ws/zlib wrappers. GC counts come from separate `PERRY_GC_TRACE=1` runs; instruction/RSS runs have tracing disabled. Kernel elapsed-time fields and Effect's two elapsed-time fields are normalized; semantic output matches exactly.

| Program | Instructions main → head | Delta / instruction noise (%) | RSS KiB main → head | Fulls main → head | Minors main → head |
|---|---|---|---|---|---|
| tsc | 9,971,062,650 → 9,971,526,952 | +0.00466 / 0.02131 | 218,628 → 217,104 | 0 → 0 | 3 → 3 |
| zod5k | 14,680,305,791 → 14,689,230,535 | +0.06079 / 0.06157 | 52,428 → 52,812 | 0 → 0 | 97 → 97 |
| qsparse | 23,501,408,052 → 23,499,678,046 | -0.00736 / 0.03111 | 57,688 → 57,644 | 0 → 0 | 131 → 131 |
| qsstr | 60,415,603,345 → 60,418,221,141 | +0.00433 / 0.04021 | 57,532 → 57,532 | 0 → 0 | 439 → 439 |
| commander | 7,562,974,564 → 7,562,572,144 | -0.00532 / 0.02436 | 53,032 → 52,908 | 0 → 0 | 32 → 32 |
| hello | 1,390,682 → 1,390,347 | -0.02409 / 0.03012 | 16,104 → 16,112 | 0 → 0 | 0 → 0 |
| fastify | 6,628,074,014 → 6,630,180,962 | +0.03179 / 0.25620 | 133,384 → 133,152 | 0 → 0 | 2 → 2 |
| effect | 24,903,168,156 → 24,909,199,781 | +0.02422 / 0.10273 | 191,596 → 190,816 | 2 → 2 | 17 → 17 |
| buffer_heavy | 10,673,094,122 → 10,670,258,459 | -0.02657 / 0.00600 | 94,016 → 94,012 | 36 → 36 | 0 → 0 |
| worker_heavy | 2,127,316,944 → 2,113,661,119 | -0.64193 / 1.30558 | 227,908 → 226,132 | 42 → 42 | 0 → 0 |
| matmul | 1,735,709,990 → 1,735,708,592 | -0.00008 / 0.00000 | 20,300 → 20,696 | 0 → 0 | 0 → 0 |
| prime_sieve | 25,201,751 → 25,201,070 | -0.00270 / 0.00031 | 18,164 → 18,176 | 0 → 0 | 0 → 0 |
| bench_buffer_readwrite | 101,604,457 → 101,603,876 | -0.00057 / 0.00002 | 18,164 → 18,172 | 0 → 0 | 0 → 0 |
| ecs_u32 | 681,978,773 → 681,978,340 | -0.00006 / 0.00000 | 18,140 → 17,692 | 0 → 0 | 0 → 0 |

All real-program instruction changes are within their current control noise floor except buffer-heavy, which improves. Buffer-heavy uses uninitialized factories before copying native output, removing redundant zero writes; an AVX2 explanatory profile (Valgrind cannot decode the normal driver's AVX512 masked instruction) confirms fewer memset and finalizer instructions. The removed detached table no longer incurs a removal probe per finalized byte cell. Hello's refreshed final-binary profile confirms 420 fewer dynamic instructions in arena teardown after deleting the 64-entry admission-cache scan; all other Perry function counts are unchanged. The smaller total reduction includes ELF/libc startup-layout effects. The kernel instruction decreases are at most 0.00270%, consistent with the same startup/teardown removal; no kernel loop code is changed by this lane.

Worker full-count trials are main [42, 42, 41, 42, 42], head [42, 41, 43, 40, 42]; both medians are 42. Concurrent transfer scheduling changes simultaneously live stores and pressure-triggered collection timing. Its RSS delta is inside the 12,856 KiB same-binary control variation, and its instruction delta is inside 1.30558% noise. Buffer-heavy retains exactly 36 full collections per arm. All full/minor medians match across the two arms.

Small RSS movements are bounded changes in linked code/data residency and allocator page granularity, inferred from the companion mapping samples and unchanged collection regime. Zod's +384 KiB, commander’s -124 KiB, hello’s +8 KiB, fastify’s -232 KiB and the kernels’ +396/+12/+8/-448 KiB accompany only small anonymous working-set changes (0–16 KiB in those samples). File-resident shifts are +180/+4/+160/+76 KiB for Zod/commander/hello/fastify, and +72/+8/+8/-128 KiB for matmul/prime_sieve/buffer-readwrite/ECS. These companion snapshots are not taken at the primary run’s exact RSS peak, so they establish the page/layout mechanism without accounting for every primary KiB. No Buffer placement rule or pacing rule changed. tsc’s -1,524 KiB is within its 2,048 KiB RSS control variation and has a 2,036 KiB anonymous companion step; the fresh THP-off check below removes that RSS difference.

Fresh current-main THP-off companion results (n=5 interleaved, `PR_SET_THP_DISABLE`, verified AnonHugePages=0 in every companion):

| Program | RSS KiB main → head | RSS control noise KiB | Instruction delta / noise (%) | Fulls / minors main → head |
|---|---|---|---|---|
| tsc | 177,448 → 177,448 | 0 | -0.00091 / 0.01627 | 0/3 → 0/3 |
| qsstr | 29,440 → 29,456 | 0 | +0.01357 / 0.05293 | 0/439 → 0/439 |
| effect | 147,724 → 147,520 | 3,116 | +0.01711 / 0.32972 | 2/17 → 2/17 |

With THP disabled, tsc's median RSS is identical across arms, consistent with the primary difference coming from huge-page granularity. qs-stringify's +16 KiB accompanies unchanged sampled anonymous RSS and +80 KiB file residency; this is bounded linked-image/page residency rather than increased byte-store allocation. Effect's -204 KiB is inside 3,116 KiB RSS control variation; its sampled anonymous/file deltas are -676/+120 KiB. All three THP-off instruction deltas are inside their control noise floors, and their full/minor collection medians remain unchanged. These companion snapshots retain the peak-accounting limitation above.

The additional eight bytes per small Buffer have not been introduced by this compatible subset; their RSS effect is unmeasured and belongs to the unified-layout change. Persistent symbols gained eight bytes each.
