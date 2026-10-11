Keylists2, 2026-10-11. Baseline: untouched main frozen at `301e9b2e114471d69957f0895ff4ef266ecac356`; candidate `8dabf349d3`. All measured instruction and GC-pause deltas satisfy the requested +0.5% limit.

Architecture: removes the canonical/class relocation scanners and minor death walks; keeps the O5-proven transition and shape caches. Canonical keys and their pointer-free attributes use existing old storage and the shared-layout movement exclusion. Full sweeps still reclaim them. Owned mutable lists and prototype words retain relocation custody; the existing writer log now survives full scans without rebuilding it over every old family. No new table, cache, latch, runtime option or per-site code was added.

Why the four programs regressed: old keys initially retained young attributes and young key strings, expanding copying/remembered work. Full collections also rescanned dirty pages after already recording all live old-parent edges during marking. Attributes now share stationary old storage; nursery key strings request promotion on their first copy using the existing tenured flag. Every barrier remains until the atom lane supplies its own immortality proof. Synchronous fulls reuse marked coverage; sliced fulls and minors retain their repair walks. Marking tests distinguish edge liveness from remembered coverage, including observer rewrites and multiple pages.

The initial private diagnostic attribution below is instructions in millions, candidate minus its matched baseline, before those fixes. It is diagnostic evidence (baseline 15b6149e5be4 plus the same rooted-value build correction), not the acceptance A/B. Allocation includes any nested collection and must not be summed with GC phases.

| Program | Census | Roots | Mark | Sweep | Dirty repair | Copying minor | Key allocation |
|---|---:|---:|---:|---:|---:|---:|---:|
| Effect | +72.670 | +3.239 | +1.300 | +40.924 | ~0 | +77.506 | +1.466 |
| schema-record | +17.049 | +3.472 | +2.237 | +27.670 | ~0 | +180.422 | +1.452 |
| buffer | +4.219 | −1.320 | +15.379 | +12.612 | +24.417 | −1.609 | +0.286 |
| worker | −1.623 | +4.562 | +18.046 | −1.214 | +29.117 | 0 | +1.132 |

Matched initial allocator-call census (logical payload bytes; includes owned lists):
| Program | Allocator calls main → head | Payload bytes main → head |
|---|---:|---:|
| Effect | 13,248 → 13,489 | 5,430,920 → 5,429,016 |
| schema-record | 13,282 → 13,509 | 5,568,776 → 5,499,512 |
| buffer | 2,328 → 2,328 | 277,424 → 277,424 |
| worker | 9,322 → 9,322 | 1,160,592 → 1,160,592 |

The allocation volume does not explain the excess. After making attributes old, Effect/schema remembered-slot visits were 455,668/474,140 versus main 7,126/10,331. Their first-copy promotion fix removes repeated survival work. Buffer/worker dirty repair visited 116,171/139,469 slots while adding zero pages beyond marked coverage. The final mark loop specializes the existing old-parent proof at object entry. Malloc parents share the ordinary loop plus one cold post-observer coverage pass, rather than a third complete loop copy. Synchronous full sweeps reuse the already completed element-shape death prune, preserving header/epoch/numeric invalidation. Copying root producers retain their decoded RootWord and publish diagnostic TLS provenance once per walk, removing repeated per-slot work. Fastify runs 29 to 42 minors after old key birth leaves its nursery cap at 4 MiB; nursery pacing remains unchanged. All root alignment, header, handle and tag checks remain. String words cannot invoke foreign handle observers, so they retain the shared decode even during an active handle trace. Child descriptors still honor that active trace, and foreign-word callbacks retain the post-mark reread.

n=5, interleaved, qb6 CPUs 0–23,32–55, ASLR off, diagnostics off. THP is disabled and verified in every row. Separate sources/targets per arm. Fresh current-main frontend objects are identical between arms (no compiler/codegen diff); pure shipping twins supply `.text`. Generated operation bytes per site are identical before and after: **0-byte change**. The shared runtime adds 3,776–3,904 `.text` bytes per executable; this comes from shared GC mark/coverage paths, rather than replication at generated operation sites. The private observer performs two atomic updates per collection, counting every worker collector and synchronous full; it is absent from shipping source.
| Program | Instructions main → head | Δ | RSS KiB main → head | Full/minor main → head | `.text` bytes main → head |
|---|---:|---:|---:|---:|---:|
| hello | 2,064,476 → 2,064,597 | +0.006% | 7,848 → 7,836 | 0/0 → 0/0 | 10,665,378 → 10,669,218 |
| tscwork | 5,669,386,312 → 5,021,650,666 | -11.425% | 166,032 → 146,904 | 0/3 → 0/2 | 89,939,666 → 89,943,506 |
| zodwork | 3,577,878,159 → 3,530,389,618 | -1.327% | 28,652 → 28,428 | 0/48 → 0/48 | 14,733,858 → 14,737,698 |
| qs_parse | 11,724,252,445 → 11,631,740,447 | -0.789% | 27,604 → 27,612 | 0/93 → 0/93 | 16,353,970 → 16,357,874 |
| qs_stringify | 18,268,611,709 → 18,055,912,681 | -1.164% | 30,304 → 29,524 | 0/225 → 0/225 | 16,361,522 → 16,365,362 |
| commander | 3,359,849,663 → 3,346,094,647 | -0.409% | 26,448 → 26,708 | 0/17 → 0/17 | 20,711,186 → 20,715,026 |
| fastify | 16,370,506,996 → 16,368,168,454 | -0.014% | 111,164 → 85,184 | 1/29 → 1/42 | 54,966,290 → 54,970,130 |
| effect | 7,725,437,648 → 6,160,588,057 | -20.256% | 134,348 → 123,472 | 1/6 → 1/13 | 41,763,602 → 41,767,442 |
| property-prototype | 446,217,159 → 446,289,199 | +0.016% | 11,616 → 11,544 | 0/0 → 0/0 | 10,777,506 → 10,781,282 |
| shapes-options | 2,600,115,575 → 2,599,396,795 | -0.028% | 30,476 → 30,588 | 0/4 → 0/4 | 10,716,322 → 10,720,226 |
| schema-record | 6,876,773,167 → 6,908,130,760 | +0.456% | 97,608 → 100,684 | 1/9 → 1/9 | 41,758,802 → 41,762,642 |
| root-survival | 1,836,158,042 → 1,809,647,003 | -1.444% | 54,020 → 54,044 | 1/18 → 1/18 | 10,717,538 → 10,721,378 |
| buffer_heavy | 9,090,710,467 → 9,046,645,316 | -0.485% | 61,500 → 60,968 | 36/1 → 36/1 | 19,094,322 → 19,098,162 |
| worker_heavy | 1,991,503,885 → 1,960,089,190 | -1.577% | 89,976 → 94,564 | 41/0 → 41/0 | 19,086,418 → 19,090,258 |
| class_getter | 46,026,200 → 46,116,672 | +0.197% | 9,472 → 9,988 | 0/0 → 0/0 | 10,712,034 → 10,715,874 |
| object_array | 32,013,558 → 32,104,340 | +0.284% | 9,964 → 10,452 | 0/0 → 0/0 | 10,719,266 → 10,723,106 |
| class_virtual | 64,545,751 → 64,590,872 | +0.070% | 11,428 → 10,876 | 0/0 → 0/0 | 10,749,282 → 10,753,122 |
| object_spread | 159,745,387 → 159,653,587 | -0.057% | 18,212 → 18,088 | 0/4 → 0/4 | 10,759,010 → 10,762,914 |

Class micro metric: (200k instructions − 50k instructions) / 150k iterations.
| Micro | Main instructions/iteration | Head | Δ |
|---|---:|---:|---:|
| class_getter | 108.0009 | 108.0003 | -0.00058% |
| object_array | 60.0011 | 60.0003 | -0.00134% |
| class_virtual | 153.5010 | 153.5014 | +0.00028% |
| object_spread | 390.3302 | 390.0795 | -0.06422% |

GC pauses: n=5 under MEASURE.lock, CPUs 56–63, ASLR off. Maximum observed sibling utilization (CPUs 24–31): 100.00%. The raw samples record intermittent sibling interference (including short micro intervals); sub-percent timing differences are within the practical noise and should not be interpreted as precise speedups. The reported n=5 medians still satisfy the requested limit. Worker pauses sum active collector durations, rather than process-wide stop time.
| Program | Main GC ms | Head GC ms | Δ ms | Δ % |
|---|---:|---:|---:|---:|
| hello | 0.000 | 0.000 | +0.000 | +0.000% |
| tscwork | 93.541 | 24.855 | -68.686 | -73.429% |
| zodwork | 15.745 | 10.861 | -4.884 | -31.019% |
| qs_parse | 21.103 | 12.729 | -8.374 | -39.682% |
| qs_stringify | 49.102 | 31.085 | -18.017 | -36.693% |
| commander | 8.393 | 6.224 | -2.169 | -25.843% |
| fastify | 80.232 | 79.962 | -0.270 | -0.337% |
| effect | 181.015 | 59.975 | -121.040 | -66.867% |
| property-prototype | 0.000 | 0.000 | +0.000 | +0.000% |
| shapes-options | 18.385 | 18.271 | -0.114 | -0.620% |
| schema-record | 64.479 | 63.611 | -0.868 | -1.346% |
| root-survival | 68.099 | 68.166 | +0.067 | +0.098% |
| buffer_heavy | 99.981 | 97.820 | -2.161 | -2.161% |
| worker_heavy | 82.068 | 77.959 | -4.109 | -5.007% |
| class_getter | 0.000 | 0.000 | +0.000 | +0.000% |
| object_array | 0.000 | 0.000 | +0.000 | +0.000% |
| class_virtual | 0.000 | 0.000 | +0.000 | +0.000% |
| object_spread | 0.673 | 0.537 | -0.136 | -20.208% |

Buffer/worker pass current main and are included in both output and performance gates; the previously reported #12091/#12092 failures do not reproduce. O5 probes remain the original evidence: transition removal costs Zod +88.2%, qs_parse +45.3%, object_spread +319.8%; shape-cache removal costs Zod +4.3%.
Full release unit suite counts and matched failures are retained in the raw logs. Both frozen-baseline and candidate suites reproduce seven shared failures: date-expando numeric representation check, two descriptor-padding checks, perex exact length, dense constructor captures, stored undefined, and typed-feedback array-literal latch. Added stationary attributes, full death, first-copy promotion and full-mark coverage/arming tests pass, with active-observer positive and negative controls. The shape writer-log test detects suppression during a full scan.
Correctness, native-corpus populations, negative controls and root-audit results are recorded separately for this exact source cohort. No older cohort's counts are inferred from these performance rows. Native C definitions and bodies join the root checker's symbol/call population; its controls reject prototypes/prose and check platform bodies and transitive calls.
Atom/intern files are unchanged. Coordination notes document the first-copy promotion request and retained barriers; the parallel atom lane is not assumed complete. No version fields changed, no push or publishing occurred.
Raw rows, logs, stress results and baseline controls: `scratchpad/codex-small/keylists2-evidence/`. Bundle: `/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/keylists2.bundle`.

The remaining schema-record instruction delta is +31,357,593 (+0.456%), with the same one full and nine minors. Separate n=3 instruction sampling corroborates the retained copying/classification and remembered coverage cost: `mark_addr` share 0.770% → 1.153%, `mark_classified` 0.747% → 0.917%, and the remembered dirty-slot callback 0.013% → 0.197%. These are diagnostic sampled shares, not exact phase counters or additive causal totals. Old-born lists and first-copy-tenured children still require classification and all ordinary barriers. The fix-forward plan is to batch canonical key/attribute births within the existing old arena and reduce repeated address classification using the current object-entry proof, preserving observer and mutable-child custody. No new cache or state is proposed.

Schema RSS is +3,076 KiB with matched full/minor counts; THP is zero in every row. Worker RSS is +4,588 KiB at a 41-full median; main spans 86,540–92,180 KiB/41 fulls and head spans 89,396–95,132 KiB/40–42 fulls. Old storage and collection high-water timing change the physical footprint; the worker samples also include scheduling variation. Full-death tests verify that stationary storage is reclaimable. These RSS increases are reported explicitly and are not attributed to huge pages.

Frozen301 validation: all 18 ordinary and pure executable outputs match Node. Candidate release runtime suite: **5,751 pass / 7 fail / 5 ignored**; untouched baseline: **5,735 pass / the identical 7 fail / 5 ignored**. The new producer-provenance and upstream generator-custody integration tests pass. The existing failures are retained by exact name in `current301-raw/units-matched.json` and both suite logs.

| Fresh native census | Main | Head |
|---|---:|---:|
| Compiled sources / skipped | 318 / 0 | 318 / 0 |
| LLVM modules | 362 | 362 |
| Functions | 9,936 | 9,936 |
| Parsed statepoints | 99,501 | 99,501 |
| Live bundles | 70,298 | 70,298 |
| Relocates | 317,539 | 317,539 |

Both native zero-budget gates remain red for the same two baseline findings: `array_append_compact::main` unrooted/global → `js_array_alloc`, and `array_record_stack_custody::closure7` stale/unmasked → `js_ctor_return_override`. All **40/40** planted violations are detected, zero missed. No hazard budgets, windows, immovable exemptions or parser waivers were relaxed. Parsed statepoints exclude module declarations; the raw corpus log's larger lexical total includes them.

All five static root audits pass: allocation-regex alternatives 76, poll-capable entries 157, macro exports 79, poll reach 4,096 exports/1,610 call-bearing exports/402 allocating symbols, and zero immovable exemptions. Fresh candidate archives define 3,938 symbols; all are visible within the C-aware scanner's 4,097-source-symbol population. Untouched main's original symbol scanner reports the pre-existing `js_back_hot_text` visibility failure that this lane's native-C definition/body parsing fixes. Controls cover body-versus-prototype/prose discrimination and transitive calls.

Root-holder audit passes: 1,503 declarations, 1,312 identity-ratcheted, 691 scanner-reached, 471 classified inventory entries, 335 enumerated frontier holders, 146 registered scanners; **92/92** controls pass. Its documented frontier limitations remain explicit. Rekey census: **41 sites, 27 prunes, zero declared gaps**, **12/12** controls pass. Pin audit: 80 tokens, three existing exceptions, **6/6** controls pass. Stock root-checker self-tests, Node-version consistency and file-size checks pass.

Untouched main reproduces the candidate's existing address-class, shape-descriptor and GC-header-constant diagnostics. Main's holder audit also reports a pre-existing `gc/mod.rs` snapshot-pin mismatch. This lane re-audits the synchronous-full custody window and refreshes that one pin; the candidate gate and its controls pass, with source-pin enforcement intact. These baseline failures are preserved in the evidence and are not called passing tests.

The measured baseline was current when rebased at 19:09 UTC and includes stdelete/classfast, genmodel, streambirth and the spread correction. Later shared-main updates were kept out of the active A/B. A merge-tree check against origin/main `06222a1ac3` is conflict-free; it does not substitute for this frozen cohort's measurements. Atom/intern files remain untouched, and the coordination note records the nursery-guarded first-copy-tenuring contract and retained barriers.

Final moving-GC stress: **18/18 outputs match Node, zero process failures**, under force evacuation, evacuation verification, protected from-space and from-space scanning, with schedule seed 1. **24,525 copying minors** and the explicit full collections are recorded per program. The allocation-free hello/property-prototype/class_getter/class_virtual runs perform no collection; moving-subject coverage comes from the allocating workloads and the dedicated stationary-backing/moving-child tests, not from those zero-minor rows. The completed-round marker and all raw logs are retained.

Cleanup verified: **42 lane-owned target directories removed; zero target directories remain** beneath `/root/codex-lanes/cx-keylists`. Raw native IR, profile data, both build/suite logs, archive hashes, fresh tsc input/compiler proof, all 440 acceptance rows, output statuses and stress counters were copied before deletion. `targets-cleaned.json` records every removed path.
