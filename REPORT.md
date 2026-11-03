# BRG DB/hash verification and delivery

Production source: ec91584e9007797711c13b4e831c62a3bf8e54b8.
Merged baseline: origin/main b75a6a8a86ffc7ff7d469d0395291306341e3727; merge, never rebase.
No push. Lane commits are by Ralph Küpper <ralph3@skelpo.com>, with no attribution lines.
Ball remains separate and requires the002/003 design review before building.

Manager009 accepts NBody's named+3.9% code-placement effect under007. The original
normal-layout positive clean cohort stays disclosed below. Relocation-checked
hot-function code matches and order controls shrink the cost to+0.3–0.4%.
No order or alignment tuning ships.

## Latest-main merge revalidation

Per manager009, revalidation is limited to the merge's build/runtime/startup/DB
scope. Both fresh main and head passed cargo build-perry with LLVM22, separate
Cargo/runtime directories, x86-64-v3 and retained symbols, then GC sidecars.
The full merged-head runtime suite passed 6204 tests, 0 failed,
48 existing ignored. Both sets of name-publication semantic tests ran.
Main's canonical publisher and decoder structure are retained, with BRG's owner
read reuse, successful-publication proof and image-name/code-key checks.
All ten DB/Symbol mechanism patches have identical added/removed lines relative
to their respective old and new baselines: brgm-mechanism-source-scope.json.

Exact startup, three deterministic runs per arm, same paths/environment, ASLR off:

| Witness | Main Ir (each of3) | Head Ir (each of3) | Delta |
|---|---:|---:|---:|
| hello | 142166 | 142166 | +0 |
| commander | 21221001 | 21217107 | -3894 |
| commander-help | 23587176 | 23583851 | -3325 |
| fastify | 164428334 | 164360914 | -67420 |
| Richards | 42769430 | 42691105 | -78325 |

Top five changed functions by absolute self-cost delta (head minus main Ir):

- hello: no changed functions.
- commander: /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/validations.rs:core::str::converts::from_utf8 -6342; ???:perry_runtime::object::native_module::class_method_declaration_value_bytes +5316; /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/converts.rs:core::str::converts::from_utf8 -1372; ???:perry_runtime::symbol::next_id -527; ???:perry_runtime::object::canonical_keys::probe_matching_node::<perry_runtime::object::canonical_keys::probe_node::{closure#0}> -330.
- commander-help: /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/validations.rs:core::str::converts::from_utf8 -10485; ???:perry_runtime::object::native_module::class_method_declaration_value_bytes +8550; /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/converts.rs:core::str::converts::from_utf8 -2254; ???:perry_runtime::object::shapes::layout_probe::probe_get_underived::<true> +1067; ???:perry_runtime::object::shapes::layout_probe::own_value::<true> +871.
- fastify: ???:perry_runtime::object::native_module::class_method_declaration_value_bytes -36440; ???:perry_runtime::object::shapes::layout_probe::derive -11550; /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/validations.rs:core::str::converts::from_utf8 -10461; ???:perry_runtime::symbol::next_id -6324; ???:perry_runtime::object::object_ops::define_property::apply_property_descriptor -4716.
- Richards: ???:perry_runtime::object::native_module::class_method_declaration_value_bytes -76185; /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/validations.rs:core::str::converts::from_utf8 -1276; ???:perry_runtime::symbol::next_id -465; /rustc/db8f076d2619ce2585b0380dda06e8da25a40da4/library/core/src/str/converts.rs:core::str::converts::from_utf8 -308; ???:<hashbrown::map::HashMap<usize, perry_runtime::builtins::fn_metadata::RegisteredFunctionSource<&[u8]>, std::hash::random::RandomState>>::insert +303.

The measurement-only input helper fixes entropy/clocks and selects an actual canonical
stack-map row with an RLIMIT gap assertion. Its exact C sources and hashes are
archived; it is outside product code. No startup noise waiver.
All startup deltas are <=0 and full self-cost ledgers reconcile. Evidence:
startup-brgm/{counts.json,diff.json,environment.json} and every raw callgrind profile.

DeltaBlue n5, taskset on one admitted idle core with idle SMT sibling, alternating
main/head order, Node26.12.1 parity on every run; all attempts retained:

| Metric | Main median | Head median | Delta |
|---|---:|---:|---:|
| instructions:u | 27647178330 | 26758242259 | -3.2153% |
| cycles:u | 5926285297 | 5845407234 | -1.3647% |
| duration_time | 1491110324 | 1474789699 | -1.0945% |

Head instructions remain below the pre-regression27,033,336,012 reference.
Raw counts, sibling utilization and attempt records: brgm-db/. Fresh-producer hashes,
new ELF hashes, build logs and a consolidated audit are in brgm-audit.json.

All retained DB pairs: all n30 paired cycles median-1.1009%, range[-22.1580,+8.3075]%; uncontended n18 paired cycles median-1.1244%, range[-2.7810,+0.1413]%; raw paired instruction/cycle/wall ranges are in brgm-db/paired-ranges.json.

Archived Cargo intermediates were restored and hash checked before this merge build; historical compressed copies remain. Current measured producers are intact.

## Earlier complete verification, retained with its source

The sections below retain the full verification of source7d66c8a0c on baseline4268659de.
That includes the full Perry/codegen/wpfacts/runtime/Windows/ABI suites, JSC22/22,
whole/per-unit tables and full tdiff equality, all14 AWFY six-pair results and the
Storage/NBody profiles and layout controls. Those results are attributed to that
integration; they were not silently relabeled as reruns on the latest main.
Manager009's explicit narrower merge revalidation is reported above.

## Changes and mechanisms

Family-method call admission uses exact carried words whose emitted immutable
method index can answer the name. Index emission and admission share the same
complete-prototype selection. Own/accessor shadows and out-of-band or non-default-prototype
rows stay excluded, and runtime receiver, holder, epoch and ABI guards remain.
Current-main GDB, DeltaBlue1/1: all execute-family probes280 ->0, all family
probes446 ->94. The first missing stack is Plan::execute -> Vector::forEach ->
c.execute. The earlier140 count was a receiver-class subset, not the full count.
The executable has96 family descriptors,25 with an index; execute exists in one
(class41). Global name availability alone cannot remove unrelated-family misses.

Fresh Symbols store the bijective MurmurHash3 fmix64 of their unique ordinal in
the existing move-stable id field, once at birth. Relaxed atomic allocation
supplies uniqueness; existing publication owners retain their roles. The inverse
preserves the defensive own-symbol enumeration fallback. No new header field.
Originally schema ordinal6/hash6 collided with _tag6; mixing moved it to bucket3.
Current main's lazy startup incidentally makes schema ordinal1; its stored hash
becomes12994781566227106604 (bucket4 with mask7). This spreads sequential identities
generically; it does not promise collision freedom or depend on a startup count.

Method construction shares the existing registered-name read. When it infers a
missing name, actual successful owned-byte publication plus UTF-8 validation
establishes the fallback name. Exact equality of the supplied registration key
and descriptor code is required. Image names, including empty ones, still win.
Failed/malformed publication and differing keys use the ordinary reader. Computed,
empty-name and native-entry modes retain their ordinary registration/read path.
The latest merge retains main's canonical full-slice register_function_name_bytes
publisher and inline decoder, rather than BRG's former private copy wrapper.
The public copying FFI API still returns void and owns its bytes. Both main's
publication tests and BRG's lifetime/owner/image/key semantic tests are retained.

No new registry, cache, runtime knob, threshold, ABI entry, TypeScript semantic
proof, or wpfacts production change. No DECISIONS211/212 guard/assertion weakened.
The initial merge deduplicated equivalent metadata helpers and the image-audit
negative-fixture correction already landed on main, retaining BRG edge coverage.

## Verification

- Codegen2589 pass,1 existing ignore; wpfacts561 pass,5 existing ignores.
  These92ff results and its facts are reused with the byte-identical final compiler
  dac3396d5bfb2b483a864e0adfba13e496fbda2a96dec860695b7ff4f2409358.
  The only subsequent production changes are private runtime metadata operations.
  Scope and byte checks: evidence/infer-compiler-proof.json.
- Final runtime6190 pass,0 failed,48 existing ignored. The new copying/inference
  test ran and passed; six image-name/source-tail combinations ran in the
  descriptor fixture.
- Perry:1326 listed,1324 active passed in fresh processes,2 existing ignored,
  0 failed. Every assertion/case retained. Fresh processes avoid the previously
  demonstrated combined-state CLI fixture interference.
- JS-compiled parity22/22 both arms against pinned Node26.12.1.
- Windows runtime offline check passed. Native ABI check has zero native
  class/width/arity/return errors; Wasm catalogue check passed.
- All52 complete whole/per-unit tables (26 witnesses in both modes,104 files)
  equal baseline after synthetic-cache-prefix normalization. Complete site
  comparisons and all26 full tdiff outputs/arm also match; no new per-unit-stronger
  site/template. Existing baseline stronger rows are explicitly retained.
  d_three_math, matmul and prime_sieve have zero templates and tdiff's documented
  NO ORACLE exit2 in both arms; these are not template-oracle passes.
- Producer/archive hashes remained stable through all affected checks and timing.

Evidence under /root/BRG/evidence: tip-suite-summary.json, infer-runtime.log,
infer-runtime-summary.json, infer-perry-fresh/{manifest.json,results.json},
infer-windows.log, infer-abi.json, tip-main/jsc/results.json,
infer-head/jsc/results.json, tip-facts-equal.json, tip-facts-comparison.json,
infer-audit.json. infer-audit.log preserves its intentional NBody gate failure.

## Native measurements

qb9, taskset, one CPU/SMT pair per case, AB/BA order for six pairs. All180
samples remain; sibling raw ticks and host load are recorded for every run.
Manager007's shared-host design chooses the quietest available pair, with every
contended pair retained and marked. The common AWFY executable selects its case
by CLI arguments. Every run matches the Node oracle.

The following n5 medians are the first five pairs fixed in advance; pair6 remains
in the paired analysis. Wall medians include recorded contention and are not
claimed as clean estimates.

| Case | Main instructions | Head instructions | Instructions delta | Cycles delta | Wall delta |
|---|---:|---:|---:|---:|---:|
| Bounce | 38,851,116,936 | 38,838,870,413 | -0.0315% | -0.9200% | -0.7409% |
| CD | 24,791,539,007 | 24,791,409,060 | -0.0005% | +0.1607% | -0.1620% |
| DeltaBlue | 27,663,987,378 | 26,775,168,630 | -3.2129% | -6.0110% | -5.9162% |
| Havlak | 92,072,777,856 | 91,409,167,492 | -0.7207% | -3.4740% | -2.9728% |
| Json | 110,261,142,403 | 110,260,530,552 | -0.0006% | -23.1704% | -23.8453% |
| List | 15,974,047,490 | 15,973,756,347 | -0.0018% | -0.5587% | -0.2819% |
| Mandelbrot | 2,792,860,414 | 2,792,782,375 | -0.0028% | -0.0228% | +0.0118% |
| NBody | 51,697,497,032 | 51,697,427,859 | -0.0001% | +4.0293% | +4.2175% |
| Permute | 52,333,642,623 | 52,333,553,196 | -0.0002% | -0.5949% | +3.3267% |
| Queens | 40,129,538,593 | 40,129,451,638 | -0.0002% | -0.0716% | -1.3330% |
| Richards | 207,754,431,318 | 207,754,205,932 | -0.0001% | -33.6923% | -37.2708% |
| Sieve | 32,637,573,766 | 32,637,584,003 | +0.0000% | +0.2379% | +0.7458% |
| Storage | 58,983,784,142 | 58,951,031,721 | -0.0555% | -17.1549% | -16.1978% |
| Towers | 48,259,316,843 | 48,259,236,214 | -0.0002% | +0.6966% | +0.7413% |
| property-prototype | 788,641,458 | 788,641,589 | +0.0000% | +1.2130% | +2.0185% |

DeltaBlue head26.775168630B instructions is below pre27.033336012B.
Property head788.641589M is+0.2673% versus pre786.539015M.
Manager003 explicitly accepted the minimal +0.27% residual under rule112's side
band. It is named here and is not represented as complete pre-number recovery.
Against current main its instruction count is flat. Property's33ms n5 wall
median is+2.02%; its six-pair cycles median is+0.531%, with the full range below.

Paired percentages are100*(head/main-1), then median/full range, not a ratio of
arm medians. A dash means no uncontended pair, not a filtered pass.

| Case | All6 cycles median % | All6 range % | Uncontended n | Uncontended median % | Uncontended range % |
|---|---:|---:|---:|---:|---:|
| property-prototype | +0.531 | -8.515 .. +11.225 | 6 | +0.531 | -8.515 .. +11.225 |
| DeltaBlue | -5.887 | -7.526 .. -4.252 | 6 | -5.887 | -7.526 .. -4.252 |
| Richards | -25.405 | -39.739 .. -17.216 | 2 | -17.785 | -18.353 .. -17.216 |
| Json | -26.498 | -47.877 .. -14.680 | 1 | -19.472 | -19.472 .. -19.472 |
| CD | +0.296 | -25.467 .. +1.250 | 5 | +0.417 | -0.544 .. +1.250 |
| Havlak | -3.401 | -5.017 .. +5.474 | 5 | -3.721 | -5.017 .. -2.484 |
| Bounce | -0.804 | -1.551 .. +0.031 | 5 | -0.562 | -1.181 .. +0.031 |
| List | -0.382 | -0.925 .. -0.080 | 6 | -0.382 | -0.925 .. -0.080 |
| Mandelbrot | -0.002 | -0.026 .. +3.615 | 5 | -0.003 | -0.026 .. +0.028 |
| NBody | +3.904 | +2.459 .. +5.702 | 4 | +3.904 | +3.424 .. +4.322 |
| Permute | -0.458 | -1.892 .. +24.942 | 3 | -0.987 | -1.892 .. -0.932 |
| Queens | -0.153 | -1.470 .. +0.590 | 4 | -0.153 | -0.533 .. +0.120 |
| Sieve | +0.176 | -0.092 .. +4.370 | 6 | +0.176 | -0.092 .. +4.370 |
| Storage | -17.249 | -17.603 .. -16.826 | 6 | -17.249 | -17.603 .. -16.826 |
| Towers | +0.535 | -0.478 .. +25.125 | 4 | +0.133 | -0.478 .. +0.913 |

## NBody: explained placement/frontend effect; accepted in manager009

Original six-pair cycles median+3.9044%, range[+2.4586,+5.7016]%; four
uncontended pairs median+3.9044%, range[+3.4244,+4.3220]%.
Instructions median-0.0001431%. The positive clean range is a real measured cost;
later contended negative pairs are not used to hide it.

Clean sampled self-cycle periods (main -> head): advance7.902B ->8.000B,
closure19 1.187B ->1.258B, dynamic_string_or_number_add269.46M ->413.36M,
class-holder chain98.25M ->170.01M, native-call tower121.18M ->178.20M.
Both cycle and Icache profile pairs had <=2% sibling activity and no lost samples.
Clean Icache samples: total49.186M ->102.453M, advance21.527M ->39.887M,
closure19 11.835M ->21.121M, dynamic add1.754M ->5.002M.

Additional clean, unmultiplexed perf-stat pairs:
Icache loads406.903M ->793.487M; misses51.150M ->105.252M.
Frontend-bound slots12.5% ->17.7%. Branch misses22.480M ->17.546M,
so increased branch-miss work does not explain the cost.
Earlier contended metric attempts remain and are not interpreted as clean data.
Admission receipts: nbody-clean-stats/{activity.json,selected.json}.

The normal advance bodies have4147 ->4145 instructions (shape clone3922 ->3920):
two instructions are removed in the tail family-admission path. The hot prefix
is otherwise unchanged after actual ELF relocations. Closure19(662), array
forEach(830), dynamic add(13), family store(375), and holder chain(788) match
between arms. The native-call tower has4454 instructions in both, with cold
literal-address differences. No extra adapter/call route is introduced.

The first control reordered only app methods and enter_home; it did not remove
the cost. A second, profile-guided CONTROL reorders the eight sampled hot
app/runtime functions lexically and in reverse, in both arms. All eight functions'
instructions and referenced constants match each arm's normal binary after
validated ELF symbol/GOT/boxed-static-pointer relocation. Numeric constants and
arbitrary immediates are not discarded. This changes placement, not the algorithm.

| Isolation/control | All6 cycles median % | All6 range % | Uncontended n | Uncontended cycles median % |
|---|---:|---:|---:|---:|
| normal | +0.567 | -19.845 .. +4.028 | 1 | +4.028 |
| compiler-only | +1.196 | -16.929 .. +3.230 | 1 | +2.434 |
| runtime-only | -0.908 | -12.249 .. +10.200 | 0 | — |
| hot-lex | +0.147 | -4.896 .. +8.882 | 1 | +0.293 |
| hot-rev | +0.648 | -2.293 .. +3.018 | 1 | +0.440 |

The clean normal isolation pair is+4.028%; hot-order controls are+0.293%/+0.440%.
Compiler-only's one clean pair is+2.434%; runtime-only has no clean pair, so that
part of the crossover is not claimed as a clean isolation proof. Every pair,
including all contended repeats, is reported. The control removing the cost and
the doubled clean Icache misses identify frontend placement sensitivity.
No function-order/alignment change ships. Source remains the generic mechanism fix.
Evidence: nbody-analysis/, nbody-isolation/, nbody-isolation-code.log,
BRG-nbody-isolation-code.py. Manager009 explicitly accepted this named placement effect under007, based on
unchanged instructions, relocation-checked hot code and the order controls.
The normal-layout positive cost remains disclosed; no order/alignment tuning ships.

## Exact startup: PASS

x86-64-v3, PERRY_KEEP_SYMBOLS=1, ASLR off; same executable/synthetic-source paths,
cwd/env per arm, three alternating runs/arm. Every triple is identical.
The measurement-only JRX input helper fixes entropy, clocks and canonical actual
stack-map row selection with an RLIMIT gap assertion; it is outside product source.
No startup noise waiver. Full per-function ledgers reconcile exactly.

| Witness | Main Ir (each of3) | Head Ir (each of3) | Delta |
|---|---:|---:|---:|
| hello | 142166 | 142166 | +0 |
| commander | 21307245 | 21299528 | -7717 |
| commander-help | 23683270 | 23671421 | -11849 |
| fastify | 165000022 | 164916265 | -83757 |
| Richards | 42986762 | 42905457 | -81305 |

Top five changed functions by absolute self-cost delta (head minus main Ir):

- hello: no changed functions.
- commander: perry_runtime::builtins::fn_metadata::copy_function_name +16253; js_register_function_name -15919; perry_runtime::object::native_module::class_method_declaration_value_bytes -5280; ./string/../sysdeps/x86_64/multiarch/memcmp-avx2-movbe.S:__memcmp_avx2_movbe -1832; perry_runtime::symbol::next_id -527.
- commander-help: perry_runtime::builtins::fn_metadata::copy_function_name +24554; js_register_function_name -24222; perry_runtime::object::native_module::class_method_declaration_value_bytes -9396; ./string/../sysdeps/x86_64/multiarch/memcmp-avx2-movbe.S:__memcmp_avx2_movbe -3167; perry_runtime::object::shapes::layout_probe::probe_get_underived::<true> +1067.
- fastify: perry_runtime::object::native_module::class_method_declaration_value_bytes -51700; perry_runtime::builtins::fn_metadata::copy_function_name +19335; js_register_function_name -19080; perry_runtime::object::shapes::layout_probe::derive -11550; perry_runtime::symbol::next_id -6324.
- Richards: perry_runtime::object::native_module::class_method_declaration_value_bytes -80585; perry_runtime::builtins::fn_metadata::copy_function_name +6035; js_register_function_name -5872; perry_runtime::symbol::next_id -465; ./string/../sysdeps/x86_64/multiarch/memcmp-avx2-movbe.S:__memcmp_avx2_movbe -432.

Evidence: startup-infer/{counts.json,diff.json,environment.json} and profiles.
The92ff source failed commander+58/help+864. The first shared-read candidate3a6
failed+4156/+10643 because its cold inference path still read the newly written
name. Profiles identified that path;7d66 uses the actual write result and passes.
All failed profiles remain. The source3a6 runtime suite passed6189; its later
unused Perry test compilation was cancelled after the deterministic startup failure.

## Prior Storage explanation requested in003

The broader closure-proof ABI candidate is excluded in full. It had Storage
+6.65% cycles. Deterministic lexical/reverse order controls changed that to
-8.79%/-6.57%, with six hot functions' normalized instructions/constants identical.
All18 accepted hardware profile runs were clean and unmultiplexed.
Frontend-bound28.6% ->37.7%; buildTreeDepth cycles1.245B ->1.649B and L1I misses
11.419M ->20.296M; family probe cycles0.702B ->0.967B and misses18.368M ->25.162M.
Branch misses34.515M ->35.263M. No layout tuning or post-proof ABI adapter ships.
Full evidence: storage-layout-clean2/profile-attempt4, layout-control/hot-code.json.
The completed334a verification and frozen bundle are separately retained;
they are not substituted for the current426 integration.

## Producers, preservation and delivery

Both own production targets used LLVM22 and /root/build.sh cargo build-perry
--release -j6, then scripts/gc_call_effects/sidecars.sh. PERRY_RUNTIME_DIR is pinned.
The final affected Cargo checks held one build slot for serial runtime/Perry/
Windows commands. Only owned, verified PIDs were stopped on rejected candidates.
No historical checkout was profiled or bisected. No push.

- /root/BRG/evidence/pre-brgm-producers/main-perry-main: 5ced4da8dd67ae2f6da3dfe21abcea74a0b1540ff3491967e91141bc3d2c75fd
- /root/BRG/evidence/pre-brgm-producers/main-libperry_runtime.a: a66b0cf91ba702193a4504d0c804a4e291f9dfc3cd48f9b093db2a29c8903fe1
- /root/BRG/evidence/pre-brgm-producers/head-perry: dac3396d5bfb2b483a864e0adfba13e496fbda2a96dec860695b7ff4f2409358
- /root/BRG/evidence/pre-brgm-producers/head-libperry_runtime.a: 405e256197dd40993f66a001d9e32f9b0bd51c5397f34b7edb6d0d96e46521d2

Complete raw timing, profiles and executables remain on qb9. Large inactive facts
and logs are losslessly gzip-compressed, with decompressed SHA verification and
path/size/hash manifests in compressed-inactive-facts.json and compressed-tip-facts.json.
Current timing/ELFs/startup profiles stay intact. Space is monitored against25GB.

Current delivery: /root/BRG/brgm.bundle, refs/heads/brg, based on the main commit
listed at the top of this report. Its docs-only head and artifact hashes are in
brgm-delivery.json and mailbox013-done.md. Prior brg.bundle remains preserved.
Ball is pending its separate exact construction-to-escape design review.
