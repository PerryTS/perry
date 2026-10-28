# BRG: DB-PROBES and SYMBOL-HASH

This is the DB/hash delivery authorized by manager replies 002 and 003. Ball is
pending a separate construction-to-escape design review before any further Ball build.
No Ball publication-model change is included.

Verified production source: f39ce2f62d9c320103249d4ac4ac8f1d7e6e0eea.
Baseline: origin/main 334a705b9 (latest when fetched for integration), merged
without rebasing in 3b066252584c2455ac76c7b839eb49cfbb3ec2e4.
All lane commits are authored and committed by Ralph Küpper <ralph3@skelpo.com>,
without attribution lines. No push.

## Changes and mechanisms

Family-method call sites admit only exact layout words whose immutable method index
can answer the requested name. Both the index emitter and admission use the same
complete-prototype selection; own/accessor shadows and non-family-band rows stay
excluded. Receiver, holder, and call-ABI runtime guards remain. No runtime index,
cache, registry, knob, threshold, ABI entry, or TypeScript semantic proof is added.
Current baseline/head GDB on the one-iteration DeltaBlue witness counts the
always-missing execute-family path 140 -> 0 (the full benchmark previously made
7.2 million such probes). See evidence/merged-mechanisms.

Fresh Symbols mix their unique ordinal once with the bijective MurmurHash3
64-bit finalizer, stored in the existing move-stable id field. Relaxed atomic
allocation supplies uniqueness; existing publication locks retain their roles.
The inverse finalizer preserves the defensive symbol-enumeration fallback order.
At the original baseline, schema id6/bucket6 collided with _tag6; mixing placed it
in bucket3. Integrated main's lazy startup incidentally moves schema to ordinal1:
bucket1 -> mixed id12994781566227106604/bucket4. The fix spreads sequential identities
generically; it does not promise collision freedom or depend on a startup count.

To satisfy strict startup after integrating main, method construction borrows
existing function metadata when it only tests presence or equality, avoiding
temporary String copies. Reflection retains owned results. Existing locks and
UTF-8, invalid-owned fallback, valid-empty precedence, descriptor-name and
optional-source-tail semantics are preserved. Tests cover those distinctions.

The integrated baseline had a stale negative image-birth fixture: it directly
invokes the unconditional audit body but expected the optional production hook's
disabled behavior. Both pristine334a and head reproduced that isolated failure.
The fixture now requires rejection in release too; production audit is unchanged.
No wpfacts production changes or DECISIONS211/212 guard/assertion changes ship.

## Verification

- Codegen: 2585 passed, 1 existing ignored; wpfacts: 547 passed, 5 existing ignored.
  The final compiler is byte-identical to the compiler used for these suites.
- Final runtime: 6175 passed, 0 failed, 48 existing ignored.
- Perry: all1318 enumerated cases accounted for; 1316 active passed, 2 existing
  ignored, 0 failed. Every case ran in a fresh process, retaining every assertion.
  This avoids a previously demonstrated shared-state CLI fixture issue.
- JS-compiled: 22/22 for both baseline and final head, pinned Node26.12.1.
- Windows runtime offline check passed. Native ABI check has zero native
  class/width/arity/return errors; Wasm catalogue check passed. The metadata
  follow-up adds no native exports or signatures.
- All52 whole/per-unit fact tables (26 witnesses in both modes,104 input files)
  equal baseline exactly after synthetic-cache-prefix normalization. Complete
  normalized site and tdiff comparisons also match; no new per-unit-stronger site
  or template. Existing baseline stronger sites remain unchanged.
  tdiff explicitly reports NO ORACLE (zero templates) on d_three_math, matmul,
  prime_sieve in both arms; these are not claimed as template-oracle passes.
  Their complete site tables are still equal. The final checker validates these
  exact three documented exit2 cases rather than accepting arbitrary errors.
- Final audit marker: evidence/names-final-audit.complete. The earlier
  names-verify3 wrapper stopped on its overbroad tdiff-exit assertion; that failure
  remains recorded. The corrected audit reuses the complete unchanged measurements.

Evidence: merged-suite-summary.json, names-runtime2.log, names-perry-fresh.log
and its per-case result manifest, names-windows.log, merged-abi.json,
merged-main/jsc/results.json, names-head/jsc/results.json, facts-equal.json,
facts-comparison.json, names-final-gates.json (all under /root/BRG/evidence).

## Native n5 medians

qb9, taskset on one admitted idle physical CPU per case, alternating paired arms,
five runs per arm. Both CPU and SMT sibling must be idle at batch admission;
sibling activity is recorded during every sample. Every interfered batch is
retained and repeated by that activity rule, never by its benchmark delta.
The common AWFY executable selects each case through its CLI arguments. Outputs
match the pinned Node oracle for every accepted run. Final ELF hashes are in
evidence/names-native-binaries.json; raw perf, CPU and parity receipts are in
evidence/names-final. Final audit checks all14 AWFY cases plus property, not a subset.

| Case | Main instructions | Head instructions | Instructions delta | Cycles delta | Wall delta |
|---|---:|---:|---:|---:|---:|
| Bounce | 39,391,363,434 | 39,379,131,159 | -0.0311% | +0.0028% | +0.7333% |
| CD | 25,186,341,224 | 25,186,060,938 | -0.0011% | +0.0608% | -0.2650% |
| DeltaBlue | 27,717,066,826 | 26,828,515,742 | -3.2058% | -1.8537% | -1.7667% |
| Havlak | 92,053,985,162 | 91,390,453,473 | -0.7208% | -2.8945% | -3.4115% |
| Json | 110,265,592,704 | 110,264,877,888 | -0.0006% | -16.3002% | -15.6320% |
| List | 16,360,091,485 | 16,359,806,562 | -0.0017% | +0.1926% | -0.1321% |
| Mandelbrot | 2,792,959,434 | 2,792,881,514 | -0.0028% | +0.1500% | +0.0918% |
| NBody | 51,697,556,963 | 51,697,475,585 | -0.0002% | -2.0485% | -2.0640% |
| Permute | 52,752,669,913 | 52,752,589,157 | -0.0002% | +0.6547% | +0.7590% |
| Queens | 40,672,890,145 | 40,672,807,784 | -0.0002% | -0.3274% | -0.9202% |
| Richards | 207,754,247,959 | 207,754,168,324 | -0.0000% | -7.5661% | -7.3311% |
| Sieve | 32,637,820,966 | 32,637,750,170 | -0.0002% | -0.2233% | +0.6615% |
| Storage | 58,983,882,833 | 58,951,020,195 | -0.0557% | -3.9963% | -4.4083% |
| Towers | 49,171,564,192 | 49,171,490,720 | -0.0001% | +0.1560% | +0.0269% |
| property-prototype | 788,615,477 | 788,617,577 | +0.0003% | +0.8497% | +0.1882% |

No new AWFY regression reaches2% in instructions, cycles, or wall time.
DeltaBlue's26.828515742B instructions are below the pre-regression27.033336012B.
Property's788.617577M is +0.2643% versus pre786.539015M; manager003 explicitly
accepted the minimal +0.27% residual under landing rule112's side band.
It is not represented as complete pre-number recovery. Against integrated main
it is instruction-flat. Absolute cycles and nanoseconds are in the full medians JSON.

## Exact startup

Both arms: x86-64-v3, PERRY_KEEP_SYMBOLS=1, ASLR off, same executable and synthetic
source paths, environment and cwd; three alternating runs per arm. Every arm's
triple is identical. The measurement-only JRX input helper fixes entropy, clocks
and canonical actual stack-map row selection (with an RLIMIT gap assertion);
it is outside shipping source. No startup noise waiver.

| Witness | Main Ir (each of3) | Head Ir (each of3) | Delta |
|---|---:|---:|---:|
| hello | 142166 | 142166 | +0 |
| commander | 21295232 | 21281539 | -13693 |
| commander-help | 23698715 | 23673683 | -25032 |
| fastify | 165165230 | 165073185 | -92045 |
| Richards | 43116410 | 43036978 | -79432 |

Top five changed functions by absolute self-cost delta (head minus main Ir):

- hello: no changed functions.
- commander: `perry_runtime::builtins::fn_metadata::registered_name_string` -39407; `perry_runtime::object::native_module::class_method_declaration_value_bytes` +32576; `mi_theap_malloc_aligned` -2150; `mi_free` -2058; `perry_runtime::builtins::fn_metadata::function_name_for_closure` -1911.
- commander-help: `perry_runtime::builtins::fn_metadata::registered_name_string` -58448; `perry_runtime::object::native_module::class_method_declaration_value_bytes` +48582; `perry_runtime::builtins::fn_metadata::function_name_for_closure` -4383; `mi_theap_malloc_aligned` -3536; `mi_free` -3381.
- fastify: `perry_runtime::builtins::fn_metadata::registered_name_string` -171562; `perry_runtime::object::native_module::class_method_declaration_value_bytes` +123546; `perry_runtime::builtins::fn_metadata::function_name_for_closure` +18328; `perry_runtime::object::shapes::layout_probe::derive` -11550; `mi_theap_malloc_aligned` -9470.
- Richards: `perry_runtime::builtins::fn_metadata::registered_name_string` -182673; `perry_runtime::object::native_module::class_method_declaration_value_bytes` +147959; `mi_theap_malloc_aligned` -12480; `mi_free` -11928; `./string/../sysdeps/x86_64/multiarch/memmove-vec-unaligned-erms.S:__memcpy_avx_unaligned_erms` -7660.

Each full per-function ledger sums exactly to its total delta. Profiles,
callgrind_annotate output, normalization inputs, counts and ledgers:
evidence/startup-final/{counts.json,diff.json}. Before the metadata change the
merged candidate increased startup by21/542/1798 Ir on commander/help/Richards;
those profiles identified temporary-name allocation. The generic fix removes
642 such allocations in Richards. Failed and raw unnormalized profiles remain.

## Excluded candidate: Storage explanation requested in003

The broader closure-proof ABI candidate is excluded in full. It had a clean
Storage +6.65% cycles result. Deterministic lexical/reverse function-order
controls changed that to -8.79%/-6.57%, with the six hot functions' normalized
instructions and referenced constants identical within each arm. All18 hardware
profile runs were clean, with unmultiplexed counters. Frontend-bound rose
28.6% ->37.7%; buildTreeDepth sampled cycles1.245B ->1.649B and L1I misses
11.419M ->20.296M; family-probe cycles0.702B ->0.967B and L1I misses
18.368M ->25.162M. Branch misses34.515M ->35.263M.
This explains the rejected candidate's code-placement/frontend effect. No
function-order, alignment, or adapter tuning ships. The final minimal source
instead improves Storage cycles3.996% and wall4.408%.
Evidence: storage-layout-clean2/profile-attempt4 and layout-control/hot-code.json.

## Build and delivery

Own targets target-main/target-head and explicit PERRY_RUNTIME_DIR roots.
Both production arms used LLVM22 and /root/build.sh cargo build-perry --release
-j6, followed by scripts/gc_call_effects/sidecars.sh. The final hashes are:

- Main compiler: 6e08352a8478e1e87d10ab1379053cdd73f94d6f8749d3b1a92d2d1b027b2015
- Main runtime: ef5afb85619ce3ee554e644393540ec0c1ee7c8838ddd3399cab85c30a674455
- Head compiler: 3e749d5554f1d549c575546a8e08e9068482f9fa7de8dabe5c65c8d49fdcaeae
- Head runtime: f5b1a283214956e06ca2922e076050b2c69721f8203bf89295a01350955fcc02

Final startup and final audit both verify archive/compiler stability.
Runtime build.rs fingerprints the full source closure, including fixture files,
so the test-only follow-up changed the archive build id; final checks use that archive.
Root free space remained above25GB (319GB at the final audit). No other lane's
PIDs were killed. No historical checkout was profiled or bisected.

Bundle: /root/BRG/brg.bundle, ref refs/heads/brg, prerequisite origin/main334a705b9.
The final docs-only commit follows the verified production source; bundle head
is recorded in the accompanying mailbox done message. Full raw evidence stays on
qb9; brg-evidence.tar.gz carries compact verification receipts and driver scripts.
No push. Next Ball action is the separate exact-chain/design review required by002/003.
