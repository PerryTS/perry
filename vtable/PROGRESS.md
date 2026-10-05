# vtable lane (retire the runtime class method table, D4)

Host qb6 (ends ~21:50Z). Worktrees: `/root/claude-lanes/vtable` (branch `vtable-s1`, target
`/root/claude-lanes/vtable-run/tgt`), `/root/claude-lanes/vtable-base` (origin/main c17090892,
target `vtable-run/tgt-base`). Scripts/logs: `/root/claude-lanes/vtable-run/` (verify.sh,
meas.sh, gap1.sh, mdiff.py, probe.sh; logs/V.SUMMARY, meas/SUMMARY, gap/results.tsv).
Design: DESIGN.md (this dir).

## Slice 1: DELIVERED, commit fe5c3309b on origin/main c17090892
Bundle: `bundles/vtable-s1.bundle` (c17090892..vtable-s1). Full summary: `V.SUMMARY.txt` (this dir).
- Every class instance method gets `<method>__eclo` (was: fresh classes only); FN_NON_CONSTRUCTOR.
  Registered in the SAME init call (`js_register_class_method_with_entry`); the entry's name is
  registered lazily when the method object is built; entries are emitted after the init chunks.
- `class_prototype_method_value_for_name(cid, m)` for an OWN method with an entry returns an
  entry-backed function object (1 capture = C.prototype ref) instead of the name trampoline.
  Inherited/entry-less members keep the trampoline (unchanged identity behavior).
- After decl prototype build + parent link: `shapes::learn_object_constfn_lanes` stamps ConstFn
  lanes on the method slots (attribute claim + link restamp dropped them).
- toString source of method objects: `class_method_entry_source_func_ptr` (node_vm).
- `codegen/method_entries.rs` split (string_pool.rs back under 2000 lines; main was 2068).
- Test: `class_registry/decl_method_slots_tests.rs` (ConstFn lane + identity + trampoline fallback).

## Next (slice plan in DESIGN.md §5)
S2 CLASS identity words + born-final prototype shapes; S3 runtime by-name method lookups via
holder shapes; S4 accessors; S5 codegen latch bytes -> holder compare; S6 CLASS_PROTOTYPE_METHODS
-> property store (fixes repro); S7 deletions.

## Slice 1 results (qb6, main c17090892 vs fe5c3309b)
- Tests: runtime 4845/0 -> 4847/0 (+2 new); codegen 2464/0 both (hygiene test anchored on `(`
  plus a new `__eclo` assertion). Gap subset 363: 0 regressions, 1 fixed
  (test_gap_2159_defineproperty_class_prototype), 0 mv failures. Matrix method/inherited/
  inherited_hoisted 110 cells: 110 identical. fmt, file-size OK; ABI check unchanged;
  gc_call_effects drift (js_register_readable_handle_predicate) and root-holders failure are
  pre-existing on main.
- Instructions n=5 medians: tsc -0.85% (fulls 82 -> 83 deterministic, OldGenBytes pacing: wt's
  old baseline is 23 KB smaller), Zod -2.81%, qs -0.04%, commander -0.08%, hello 0.
- Rows (instr/iter): value_call 7659 -> 5419, proto_call 8971 -> 6731, bind 7819 -> 6290;
  own/inh3/inh6/poly4/mega10/getter6/fresh/inst6/fnctor/10498/arrsub flat (+-1).
- Startup fixtures (300 classes x 8 methods): declared-only +0.57% instr (+73k = ~30 instr/method
  of load-time relocation), RSS(THP off) +0.95 MB; touching all prototypes -9.6% instr, RSS -1.1 MB.
- RSS THP off: tsc/Zod/qs/commander flat. THP on (host default): Zod +2.0 MB, a 2 MB huge-page
  step (page faults equal, 647 vs 646).
- Size: per declared method ~290 B (one JsFunctionInfo + entry); Zod +45 KB, tsc -8 KB.

## Slice 2: BUILT + VERIFIED, NOT PUSHED (2026-10-04, perrymaster)
Head f5aaf5308 (tested tree 1fdd4d9ca, same code; only message + changelog differ) on main 2026ecfe6 (incl #11834).
Worktree /root/claude-vtable2/wt (branch perf-class-prototype-fast-link), base /root/claude-vtable2/base,
targets run/tgt, run/tgt-base. Bundle: vtable/vtable-s2.bundle (origin/main..HEAD) here and run/vtable-s2.bundle.
Logs: run/logs/V.SUMMARY (final), run/old-main-run/ (pre-rebase run), run/q2/ (controls).
- Link: class function object capture slot 1 = C.prototype; forward = class-value table loads + slot load;
  reverse = header class id + one link compare (try_read_gc_header; the compare proves the answer).
  DeclPrototypeTable + its GC scanners deleted. Born-final: decl_prototype_birth.rs + shapes_linked_birth.rs.
- Gates: fmt ok, gc_call_effects ok, wasm-abi ok, file-size only parent_static.rs (main), root-holders red on both.
- Tests: runtime 4850/0 -> 4846/0 (8 DeclPrototypeTable tests deleted, 4 new), codegen 2473/0 both.
- Gap subset (class|method|proto|instanceof|super|static, 263): 0 regressions, 0 mv failures. Matrix 110/110 same.
- Not opened: micro rows bind +0.54%, fresh +2.4% (GC pacing), inherits_hit +1.9% (rodata alignment), tsc fulls 80->82.
Next: owner/coordinator decision on those rows; then push + PR (fragment PENDING-class-prototype-fast-link.md).

## Slice 3: BUILD PHASE STOPPED, NOT OPENED (2026-10-04 ~00:20Z, perrymaster)
Clone /root/claude-vtable3: wt (branch perf-class-method-lookup-shapes) and base (detached origin/main ad6035666),
targets run/tgt and run/tgt-base, scripts in run/ (buildarm.sh, meas3.sh [real|rows], matrix.sh, gap.sh + gap1.sh,
ibsdiff.sh = per-symbol IBS op-sample diff (AMD: `perf record -e ibs_op/cnt_ctl=1/`; instructions:u sampling skids badly
and valgrind dies on AVX-512)). WIP bundle vtable/vtable-s3-wip.bundle (origin/main..3e45b7887, 2 commits:
1958ed8ca node_stream test lifetime fix (main's runtime TEST target does not compile), 3e45b7887 wip).
Base worktree carries the same test fix uncommitted (test-only).
Side events: main was unbuildable after #11843 (conflict markers in class_value.rs); my hotfix #11847 force-merged
(capture 0 class id, 1 evaluation state, 2 prototype link); coordinator kept that layout, #11848 = root-holders JSON only.

What the WIP does (runtime only, plus one codegen change):
- native_call_method/class_holder.rs (new): by-name call of a class instance's method = walk of prototype SHAPES
  (atom identity then bytes on each holder's key list; accessor/dictionary/exotic -> ordinary [[Get]] from that holder).
  Fast path at the top of the tower (replaces obj_dispatch_ic), the tail vtable arm, handle_methods' vtable_ic + parent
  walk, collection_methods' raw-pointer walk, the INT32 ClassRef arm. ConstFn lane of the holder shape names the body
  (no closure re-validation). Private (`#`) and symbol-alias (`@@`, `__perry_`) members keep the vtable via
  call_non_property_member (they are not string-keyed prototype properties).
- Deleted: VTABLE_IC, OBJ_DISPATCH_IC (+tests), note_class_vtable_resolution, instance_class_prototype_object.
- method_site: prime_inherited admits declared-class instances (holder = class_link), depth 1.
- Atom ABI: static dispatch descriptor gains a 5th word = address of the entry's pool handle (the atom) when the
  module's strings are process-wide (no workers), else null; PerryStringRef.atom_slot, read lazily (slot_atom).
  native_call_method_with_atom / typed_feedback_native_call_method_with_atom thread it.
- Not touched: bound.rs / proto_dispatch trampoline (S7), disposal.rs (symbol-alias names), super calls, Q2 readers.

Results so far (instr, base -> wt):
- Matrix 110/110 identical. Gap subset (class|method|proto|instanceof|super|static|call, 305): 0 regressions after the
  #11700 fix (re-checked that test + related); mv 0 failures. Runtime/codegen test suites, gates, sabotage NOT run yet.
- tsc +0.08%, Zod +0.29%, qs +0.07%, commander +0.05% (n=5, before the last speedups; fulls 82/82); RSS flat; sizes -4..-8 KB.
- Rows equal except: vt computed 1534 -> 2737, vt3 computed1/3/m 1534 -> 1727/1981/2444, fnctor decimal_class
  1909 -> 2081, bindb ctorbind +82. Everything else (own/inh/poly/mega/any*/levels/generic/10502/10503/10504/bind) flat.

THE OPEN PROBLEM (why not opened): the deleted (class id, name) caches made a runtime by-name hit O(1) (~60 instr).
The shape walk costs ~100 instr for the start link + ~100-125 per hop (slab record decode, key-list header, gc header,
proto word), so every runtime by-name call is slower, by depth. Decimal_class is the by_id_learn miss edge of a codegen
direct arm (latched by `Dec.prototype.plus = ...` elsewhere), computed is `obj[key]()` (no site). The design answer is a
per-SITE memo ("receiver word -> hop ShapeIds -> holder ShapeId -> slot", hops validated by ShapeId, no global word):
  (a) MethodSite gets a runtime-only far-memo (deep holders, megamorphic ways), checked in js_method_site_miss;
  (b) computed calls need a site (codegen early_branches.rs: pass a site slot to a new js_native_call_method_value entry);
  (c) the by_id_learn direct-site miss edge needs a memo slot next to its learned word (codegen direct_method_guard.rs).
Next phase (fresh agent): build (a)-(c) on the WIP, then verify (tests RUST_TEST_THREADS=1, codegen, gates, sabotage of
the holder-shape check, gc_call_effects --check, wasm-abi), full meas3.sh, then PR.

## Slice 3 build phase 2 (2026-10-04 00:20Z-07:00Z, qb6) — chain memo
Worktree qb6 /root/claude-lanes/vt3 (branch perf-class-method-lookup-shapes, base main 6235411f2), base arm
/root/claude-lanes/vt3-base, WIP-only arm vt3-wip (tgt-wip). Run dir /root/claude-lanes/vt3-run: buildarm.sh, meas3.sh
[real|rows|all], verify3.sh (wt side; base logs reused from logs/), gap1.sh, zq.sh (Zod/qs n=5 base/wip/tag), symdiff*.sh,
patch scripts p1..p11. Verified commit 38917de51 (bundle vtable/vtable-s3.bundle; scripts+summary vtable/s3-logs/).
Design as built: method_site/chain_memo.rs — per-site memo, 2 ways x up to 8 hops: receiver word, each hop's word
(strong roots, scan_chain_memo_roots_mut), holder inline slot + ConstFn body, computed key bytes; 16 replacements -> stops
recording; no memo once workers exist. (b) computed calls: @perry_cmemo_N + js_native_call_method_{str_key,value}_memo.
(c) by_id_learn: memo slot is the word after the learned word ({i64, ptr} global); guard-set edge passes site|1 (no learn)
— keeps IR equal to main (matrix 110/110). (a) method-site memo DROPPED (cost qs ~9M, no measured gain; coordinator ok).
WIP atom plumbing removed (Zod +0.07% -> -0.08%).
Verified on 38917de51: matrix 110/110 identical; gap 351: 0 regressions, mv 0 fails; runtime 4845/0 (7 deleted cache
tests); codegen 1 flaky env-race test (static_constfn, passes alone); fmt/gce/wasm-abi ok; lint: nothing new vs main;
sabotage (skip intermediate hop checks) -> new gap test test_gap_10502_class_method_chain_memo.ts red (78 lines).
Latest uncommitted state rows (instr): obj[key]() d1/3/6 1306 -> 763/782/811; decimal_class 1478 -> 827; Zod -0.07%;
qs +0.042% (accepted, decision 68); value_call/proto_call/bind +48 (NOT accepted: appeared when atom plumbing removed).
Next: fix the +48, re-run verify3 + sabotage + n=5, rebase on current main, push + PR (Refs #10502).

## Slice 3: PR #11902 OPENED (2026-10-04 ~10:20Z)
Head 73929f2a3 (code 6ea65e8af + fragment rename) on main 9e901a02f; branch perf-class-method-lookup-shapes;
bundle vtable/vtable-s3.bundle (9e901a02f..head). Full summary vtable/s3-logs/V4.SUMMARY (verify4.sh, serialized, -j12).
The +48 on value_call/proto_call/bind: found by gdb single-step counting of one js_method_site_miss
(s3-logs/stepcount.py: per-function instruction counts, deterministic). Cause: inlining in the dispatch tower changed
(state(), js_nanbox_get_pointer, the object()/jsval() closures were no longer inlined). Fix: the tower body is
`native_call_method_tower(.., memo)` (memo check on top, fast call records into it); the WIP's class-instance and
class-ref tail arms moved out of line (class_holder::call_class_instance_member / call_class_ref_method).
Result: value_call 5413 -> 5382 (-31).
Final (main 9e901a02f -> PR): obj[key]() d1/3/6 1275 -> 753/771/800; decimal_class 1447 -> 785; value_call/proto/bind
-31; fresh +29 (+0.09%); tsc -0.11%, Zod +0.05%, qs -0.01%, commander -0.17%; matrix 110/110 identical; runtime
4873/0 (main 4880, 7 deleted cache tests), codegen 2487/0; gap 356: 0 regressions, mv 0 fails; fmt/gce/wasm-abi/
file-size ok; lint 18 fails vs main 19, none new; sabotage (skip intermediate hop checks) -> new gap test red (78 lines).
Open: Zod +0.05% and fresh +0.09% (both small; coordinator accepted qs +0.04% under decision 68).
qb6 cleanup done: vt3-sab, vt3-wip and their targets removed; vt3 / vt3-base / vt3-run kept.

## #11902 rebased (2026-10-04) onto main 3847093a2 -> head aadc9fa90 (code a166e8e47)
Conflict: method_site.rs module declarations only (main added `mod function_intrinsic;`, #11893/#11885 family); kept
both. No conflict markers. cargo check --tests ok; runtime 4890/0; fmt/gce/wasm-abi ok; gap10502 normal+mv PASS;
matrix method 84/84 identical; rows: decimal_class 1447->785, computed d1/3/6 1276->753/772/800, value_call/proto/bind
-31, fresh +4; tsc n3 +0.02% (noise: wt runs 59725-59791M vs base 59758-59791M), Zod n3 flat. gc_runtime_root_holders
red identically on main (PASS1_MARKED pin for gc/mod.rs + cycle.rs, stale PRIVATE_MARKER_CACHE): main's, not ours.
Logs: qb6 vt3-run/logs5/SUMMARY; script s3-logs/post-rebase.sh.

## Slice 4: class accessor holders (2026-10-05)

Requested base: `4c87719783bc7558377e736714d1d6770619aaba`.
Integration main: `4cf05cbe389666712c4899cfe128c975fa593e0b`.
Branch: `perf-class-accessors-shapes`; code: `7728163312` (following
`fe0b12c151` and `52163c8f1a`); notes commit changes Markdown only.
Mac tree: `/private/tmp/cx-vt4`; qb6: `/root/codex-lanes/cx-vt4`.
Both arms use separate source/target directories, identical release package
and feature sets, 16 codegen units, LLVM 22 and Node 26.5.1. Compiler and
runtime/stdlib archives are rebuilt together; runtime paths are exported
after `bash -lc`. Real-program compilation uses 16 LLVM unit workers.

Read-only GitHub coordination confirmed #11925 merged at
`5d37d8f5d8985b0cf747c9f14a8e2e65cc8f1021`: its plain-closure and builtin
getter entry ABI is retained. #11902 and #11967 are in the requested base.
**No functions in `method_site.rs` changed.** Only its `read_holder` and
`read_holder/class_read` submodules changed; the function-intrinsic step is
untouched. The #11872 declared-getter-name emission gate remains intact.

Implementation:

- Q3 follows the instance's actual shape link and each holder's shape key,
  attribute and accessor-pair lanes. Declared metadata initializes prototypes
  but no longer answers accessor reads. Own and nearer data properties shadow
  farther accessors.
- Deep getter and setter sites record up to three intermediate holders in
  existing bounded per-site records. Hits check every intermediate ShapeId,
  the terminal holder ShapeId and its pair. The emitted ABI prefix remains
  intact; deep entries route through collecting hits and have strong roots.
- Removed class accessor predicates, duplicate callable-getter dispatch,
  prototype getter bypass, typed-field class accessor probes, and the class-id
  store-verdict registry / vtable-generation dependency. Negative store
  proofs depend on mutations of the materialized, marked prototype holders.
- Accessor discovery skips shapes without accessors and decodes names only
  for accessor attribute entries. Cumulative prefix summaries skip the leading
  data-only entries by binary search. A positive candidate confirms the complete
  lookup and checks nearer data shadowing. There is no new side table,
  registry, latch or name-based hot-path exception. Negative generic walks
  allocate no handle scopes.
- Generic accessor lookup uses one GC suppression scope covering both
  prototype materialization and traversal; redundant nested scopes are removed.
- `class_read::pinned_answer` is explicitly inlined to preserve the ordinary
  inherited-read instruction count.
- Native prerequisites: Request/Response prototype getter pairs execute
  native property bodies, including on subclasses; EventEmitterAsyncResource
  internal descriptor seeding defines own properties. ClassBody methods also
  define own properties, preventing inherited setters intercepting definitions.
  Accessor walks decline the prototype builder's temporary self link.

Coverage includes a getter replaced by Object.defineProperty, inheritance
through three levels, a subclass setter, nearer shadowing, relinks, runtime
accessor installation and a data holder with an unrelated getter. Runtime
units check intermediate ShapeId invalidation and late setter installation
revoking a negative store proof. Main fails the new fixture's own-data write
following a relink (`7 false` versus Node's `8 true`).

Sabotage: removing only the terminal holder ShapeId comparison in
`accessor_entry_hit` (retaining receiver/hop/pair checks) made
`class_accessor_rechecks_same_shape_holder_link` fail with a stale getter
(exit 101). Restoration passed and reproduced byte-identical compiler and
runtime/stdlib archive SHA-256 hashes on the earlier `1c3c398658` integration.
The final source retains the comparison.

Final micro rows: n=5 medians of differential 100000/1100000-iteration
instruction totals; every output matches Node at both sizes.

| Row | Main instructions/iteration | S4 | Delta |
| --- | ---: | ---: | ---: |
| lit_get_set_has | 62.999880 | 63.001385 | flat |
| inherited getter | 13379.500838 | 462.998528 | -96.54% |
| class getter | 123.997502 | 124.001166 | flat |
| subclass setter | 15506.111373 | 284.999250 | -98.16% |

Final gap subset: 183 tests, main 182 / S4 183 PASS, 0 regressions and
0 moving-GC failures. Final matrix: all 110 outputs match Node; 98 valid and
12 intentionally hoisted rows retain their statuses and every instruction
count matches main exactly. The standard diff helper refuses a 2.39% symbol
count change (29129 -> 29824; binary size +0.21%). Both arms use the same
four-package prebuilt modes and disabled auto-optimization. A separate
comparison checks all keys, statuses, outputs and instruction counts. Original
headers and helper refusal remain in matrix-diff-prefix.log; the successful
comparison is in matrix-verified-prefix.log. No headers or data were changed.

Final four-crate Rust suite (including integration/doc targets): runtime
4995 passed / 0 failed / 13 ignored; HIR 914 / 0 / 3; codegen 2509 / 0 / 6.
Stdlib's other 242 unit tests pass. Two failures reproduce on pristine main
with matching diagnostics: thread_exit_releases_the_threads_dom_exceptions
(dead thread's DOMException brand remains, exit 101), and
pipe_through_pair_survives_a_moving_getter (TypeError: Invalid transform
writable, exit 1). They prevent the requested all-green stdlib result. The
candidate pipe failure was also rerun with --nocapture to verify the exact
main diagnostic. These unrelated baselines were not changed.

Final source checks: fmt and Node-version consistency PASS; ledger/raw-debt
no-ceiling-raise checks PASS. Both census gates fail identically on main/S4:
ledger 204 tables / 188 producers, with the same zero ceilings in
perry-ext-zlib/src/stream/agent_state.rs and perry-stdlib/src/zlib/tables.rs;
raw debt 870 against baseline 868, with the same four per-file violations in
node_stream/async_iterator.rs, node_submodules/zlib.rs,
object/field_get_set/exotic_named_read_tests.rs and thread/clone_read.rs.
No S4 debt, violations or ceiling edits.

Real programs: every one of the 60 measured executions matches Node 26.5.1
byte-for-byte. Five interleaved samples per arm, user-mode instructions,
maximum RSS in KiB and PERRY_GC_DIAG=1 full-collection counts. TypeScript
5.8.2 transpiles 400 generated functions/classes three times; Zod builds and
parses schemas for 200 iterations; commander/qs/hello use the supplied gc3
drivers. Both arms compile with 16 LLVM unit workers.

| Program | Main instructions (M) | S4 instructions (M) | Delta | RSS main -> S4 (KiB) | Fulls main -> S4 |
| --- | ---: | ---: | ---: | ---: | ---: |
| tsc | 28607.047825 | 28563.133815 | -0.1535% | 264244 -> 264868 | 1 -> 1 |
| Zod | 806.094537 | 783.924988 | -2.7502% | 57448 -> 57776 | 0 -> 0 |
| qs parse | 29106.725254 | 29095.324116 | -0.0392% | 58384 -> 59004 | 0 -> 0 |
| qs stringify | 76666.239247 | 76364.007771 | -0.3942% | 60340 -> 61604 | 0 -> 0 |
| commander | 9594.842215 | 9593.409054 | -0.0149% | 53016 -> 53344 | 0 -> 0 |
| hello | 1.320835 | 1.320649 | -0.0141% | 17612 -> 17528 | 0 -> 0 |

All six instruction medians are below main. The smaller deltas are close to
measurement variation; these are the requested n=5 results. RSS is slightly
higher for five workloads, while full collections remain unchanged. All
compiled-program artifacts retain their coherent-build SHA-256 hashes after
the test/measurement run. No performance headers or samples were edited.

Completion limits: the two matching main stdlib failures and unchanged
ledger/raw-debt census failures prevent all requested gates being green.
Correctness comparisons have zero S4 regressions. The matrix standard helper
refusal is preserved alongside the complete direct comparison.

Delivery: code commits fe0b12c151, 52163c8f1a, caea87c8c0 and 7728163312,
plus the final notes/changelog commit. Bundle:
/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/vt4.bundle.
Host build targets are removed after copying the final logs.
All raw logs and harnesses are retained in the approved scratch directory:
`/Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/vt4/`.


### S4 current-main revalidation and regression fix (vt4b, 2026-10-05)

Rebased all five S4 commits from PR head 49b1629553 onto the requested main
495fa8f94984177d945707a95ab0ef25aa4acf85, cleanly. The runtime fix is
cceb932933. Versions are unchanged. This section records the current-main
check; the preceding S4 measurements describe its earlier integration.

Independent source workspaces and targets on qb6 contain main and the
rebased head. Both use the standard four-package release build with -j 12,
release codegen-units=1 and thin LTO. Compile drivers use Node 26.5.1,
TypeScript 5.8.2, PERRY_ALLOW_PERRY_FEATURES=1, PERRY_NO_AUTO_OPTIMIZE=1,
PERRY_NO_CACHE=1, PERRY_KEEP_SYMBOLS=1, one module worker and twelve LLVM
unit workers. LLVM worker count controls concurrency, not unit partitioning.
Fastify's no-auto HTTP runtime/stdlib/wrapper graph is also built separately
for each arm. No target is shared between arms. Only driver/package files
were copied; the supplied node_modules/.cache was excluded.

The reported tsc +4.15% and +8.2 MB do not reproduce with these builds.
Before the new fix, the rebased S4 is +0.215% instructions on tsc, with
identical stripped size and 856 fewer text-symbol bytes. A second cold
comparison builds historical main a6c147b7b0 and applies the original S4
diff to that source: tsc is +0.043%, stripped size is identical, and its
text-symbol total decreases by 372 bytes. The original 130.59 MB main
artifact has no counterpart among these fresh builds. The original anomaly
cannot be attributed without its baseline artifact/build settings; it is
not established as an interaction with #12041 by these reproductions.

| Cold comparison before the fix | Main instructions (G) | S4 (G) | Delta | Main stripped bytes | S4 stripped bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| current main 495fa8f949 | 28.121576 | 28.182168 | +0.215% | 139083128 | 139083128 |
| historical main a6c147b7b0 | 28.244683 | 28.256928 | +0.043% | 138853816 | 138853816 |

nm -C -S --size-sort comparisons cover all defined text symbols; section
sizes are retained as well. All 30938 distinct TypeScript-named text symbols
have exactly the same sizes in current main, rebased S4 and the final fix:
92720856 bytes. There is no reproduced codegen arm or per-site inlining
explosion. In the final binaries, .perry_src is 9099839 bytes and
.perry_gcmap is 5081675 bytes in both arms. Final text-symbol totals are
112834465 -> 112834944 (+479 bytes), and .text is
112618290 -> 112618994 (+704 bytes).

Fastify does reproduce a +3.127% instruction regression. Its instruction
profiles identify extra accessor discovery and a larger share of collection
and layout-table work. The fix has two parts:

- Let #12041's class data/absence holder proof answer before an accessor
  discovery walk. Generic getter fallback only walks an unresolved bare
  CLASS shape link: ordinary/recorded chains already use the existing full
  inherited read. These changes reduce Fastify to +2.667%.
- Fetch's new native getter implementation captured a property-name string
  in each of 25 closures. These constant names add 25 live strings and 200
  closure bytes to bootstrap, although Fastify does not need those getter
  bodies. Replace them with zero-capture bodies naming their native property
  directly. The bodies use the native byte-name property dispatcher and have
  correct reflected getter names. Fastify becomes +0.120%.

The first old main/S4 minor collection differs by exactly 25 objects and
1152 bytes (33377/2415064 -> 33402/2416216). In the final diagnostic pair,
both arms copy 33375 objects / 2414992 bytes. GC self-profile share is
7.89% main / 7.98% final, versus 9.73% for the first fix that retained the
captures. Collection counts and budgeted work units agree in the original
pair; this is increased collection/table cost rather than extra collection
frequency. The ablation and removal of the redundant captured state isolate
the reproduced remaining Fastify cost. The final design still answers class
accessors through live holder-shape lanes and retains the deleted class-table
accessor predicates/registries.

Final measurements are medians of five interleaved user-mode instruction
samples per arm. All 90 measured executions match Node byte-for-byte. Each
row meets the +0.3% limit.

| Program | Main instructions (G) | Final S4 (G) | Delta |
| --- | ---: | ---: | ---: |
| tsc (1 transpile) | 10.192972 | 10.213529 | +0.202% |
| tsc (3 transpiles; reported workload) | 28.119707 | 28.174057 | +0.193% |
| tsc ×3 (9 transpiles) | 84.969483 | 85.122413 | +0.180% |
| Zod ×5000 | 16.121007 | 16.110668 | -0.064% |
| qs parse | 28.551488 | 28.557220 | +0.020% |
| qs stringify | 76.436748 | 76.445620 | +0.012% |
| commander | 7.811391 | 7.809711 | -0.021% |
| hello | 0.001247 | 0.001248 | +0.086% |
| fastify | 108.325679 | 108.455861 | +0.120% |

| tsc size | Main bytes | Final bytes | Delta |
| --- | ---: | ---: | ---: |
| unstripped | 171790112 | 171799016 | +8904 (+0.0052%) |
| stripped | 139083128 | 139087224 | +4096 (+0.0029%) |

Both binary-size comparisons meet +0.2%. Compiler, runtime and stdlib
archive hashes are unchanged across the final measurement run.

The accessor micro rows use five differential 100000/1100000-iteration
samples per arm, with all 80 outputs matching Node:

| Row | Main instructions/iteration | Final S4 | Delta |
| --- | ---: | ---: | ---: |
| lit_get_set_has | 62.999658 | 62.999262 | -0.0006% |
| inherited_getter | 13090.511226 | 463.001443 | -96.4631% |
| class_getter | 123.999460 | 124.000171 | +0.0006% |
| subclass_setter | 15292.125483 | 284.000653 | -98.1428% |

The getter/setter/accessor/class gap subset contains 181 fixtures:
main 179 PASS, final 181 PASS, zero regressions. The two main failures are
the existing S4 own-data-write/relink fixture and the new Fetch getter-body
fixture. The other new fixture covers recorded links, getter results of
undefined, keyless receivers and data shadowing/deletion. These are default
runs; the extra forced-moving harness mode was not run in this lane.

Final runtime/HIR/codegen tests include unit, integration and doc targets:

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| runtime | 5055 | 0 | 13 |
| HIR | 921 | 0 | 3 |
| codegen | 2500 | 15 | 6 |

All 15 codegen failures are native_proof_buffer_views cases reproduced with
identical test names on pristine 495fa8f949 (that target: 32 PASS / 15 FAIL).
They concern typed-view proof diagnostics, so the requested all-green
codegen gate remains unmet. No new runtime/HIR/codegen failure is introduced.

Holder ShapeId sabotage is red. Before the new fix, removing only the
terminal comparison from source made
class_accessor_rechecks_same_shape_holder_link fail, and source restoration
passed with byte-identical compiler/runtime/stdlib hashes. On the final
compiled runtime unit test, a private copy removes only the six-byte
terminal-holder ShapeId rejection branch (0x1427ed0; receiver, intermediate
hop and pair checks remain). The stale-getter test exits 101. The untouched
executable passes before and after with identical SHA-256
14696c13de6f7a9a4234f3e5cd0b20b69d96e450300656f2121c2df5ae3baebd.
The repository comparison remains intact. Final fmt, git diff --check and
Node-version consistency checks pass.

Evidence: separate-arm release logs, raw n=5 samples, nm/section inventories,
perf instruction records/diffs for current/historical tsc and current/final
Fastify, GC diagnostics, fixtures, full Rust logs, hashes and sabotage scripts
are in /Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/vt4b/.
The final tsc/Fastify binaries and symbol/build inventories are also preserved
as compressed archives there. All eight build target directories were removed
after preservation; no target directories remain in the lane's host workspace.

Completion limits: the original +4.15%/+8.2 MB anomaly is not reproduced or
attributed without its original baseline artifacts/settings; the 15 matching
main codegen failures prevent an all-green codegen result. All requested
final instruction/size gates and correctness comparisons pass.

Bundle: /Users/amlug/projects/perry/secret-tests/scratchpad/codex-small/vt4b.bundle.
