# WASI 0.3.0 completion

Objective: complete supported WASI 0.3.0 hosting and Perry-authored component
output, including reusable typed plugin interfaces. The implementation branch
is integrated with main `63cb3a9535b860161fd7bc0f887265dec83fefe4` as of November 2.
The resource/ALS milestone landed in PR #13370. Existing historical receipts
retain their original scope. An implemented helper or passing narrow test does
not establish completion of a row below.

The scalar resource cancellation receipt qualifies the earlier integration base
`034939356199d1cb47f3ac303fafd1de8c6223d2`. The later compound receipt includes
fresh coherent builds and execution on their recorded integration base. The
[experimental resource and ALS receipt](validation/wasi-experimental-resources-als-20261028.json)
then qualifies the actual experimental entry point and runtime relocation on
Linux at source `456ebaf95d4b9313de0de9e1ef15b9ccdbf4e328`. Later Android-only
stub cleanup is source-audited separately and does not claim Android execution.

The [argument-root receipt](validation/wasi-guest-argument-roots-20261028.json)
qualifies explicit release of generated temporary inputs after the first call,
while retaining JS captures, results, task ALS and active borrow lifetimes.
It records 111 normal-mode component runtime tests, 28 WIT tests and actual
plain/instrumented async and resource regressions at `c84fb548a86343cf68b65e8b295e1edabe5e816e`.
This is a prerequisite for collecting abandoned guest endpoints; it does not
implement future/stream values.

The [P3 entropy/runtime receipt](validation/wasi-p3-entropy-runtime-20261101.json)
qualifies both pinned RNG backports in an actual Rust P3 command through the
Turnloop host, with 256 alignment/length cases per version. Binary parsing
confirms all 15 external WASI imports are 0.3.0; real P1 and P2 controls are
rejected. The production P3 runtime archive also builds. This does not qualify
Perry-generated commands, guest endpoints or all provider interfaces. A
distinct `--target wasip3` compiler/linker profile is now implemented on this
integration branch. The [command-output receipt](validation/wasi-p3-command-output-20261101.json)
records 51 passing Linux phases: nine actual Perry-generated P3 programs with
exact output/exit checks, 0.3.0-only external contract checks, and command,
clock, stdio and failure controls through ordinary and forced-moving native
hosts. Native copying/evacuation/protection witnesses are recorded; this is
not forced-moving Wasm guest proof or distinct instrument-free native builds.
Guest endpoints, plugins, complete providers and release qualification remain.

The separate [Wasm guest GC receipt](validation/wasi-p3-guest-gc-20261101.json)
records ten passing phases with native host GC controls off. Actual P3 guest
objects relocate across Promise awaits, a timer and exception handling; 13,883
copied objects and evacuation verification are observed. Distinct plain and
instrumented archives pass their off controls, and the plain archive rejects
an instrumentation grant. This closes the recorded command guest-GC gap, not
qualification of subsequent runtime changes or the remaining completion rows.

The integration branch now also implements experimental owned guest reader
capabilities and generated typed future/stream transport. Normal debug-mode
tests cover joint resource/endpoint custody, nominal payload types, duplicate
transfers, actual relocation, queued disposal and finalization. Guest reads,
writes and factories behind async exports are implemented experimentally and
qualified on Linux below. Public generation stays guarded.

The [endpoint transport receipt](validation/wasi-p3-endpoint-transport-20261101.json)
now qualifies that experimental library profile on Linux: 15 acceptance phases,
plain/instrumented execution, a separate maintained-C canonical WAIT/resume
control, and 5,346 copied Wasm guest objects with evacuation verification.
Generated frame calls keep the JS agent in persistent TLS and restore SDK task
TLS before returning to C; scheduler userdata no longer overwrites the SDK's
stack context. Thirty-two WIT/C tests and all 222 native component-host tests
pass, including five WASI capability checks.
Moving diagnostics use an explicit bounded guest stderr capture; native host
GC controls remain off. That receipt does not qualify guest reads, writes,
writer factories, the private producer actor or I/O scheduling from sync exports.

The later [guest-read receipt](validation/wasi-p3-guest-reads-20261101.json)
qualifies actual JS reads through the generated C scheduler on Linux. All 15
acceptance phases pass for plain, instrumented and forced-moving Wasm guests,
including independent host writers, blocking reads, compound u64 values,
characters, void/unit/byte payloads, nested futures, partial chunks and EOF,
pending disposal, abandoned-read retry and Store reuse. The guest copies 8,799
objects with evacuation verification; native host GC controls remain off.
Thirty-three WIT/C tests include 26 sanitizer ownership cases and invalid
future-result controls. All 122 normal debug-mode component runtime tests pass,
including collecting Promise-init hooks and creation/observer ALS isolation
through cancellation. These native sanitizer cases cover owned resource
payloads; actual resource-bearing Wasm I/O remains to qualify. Future results
are decoded without stream progress counts, and component imports select
BigInt operations even without BigInt expressions in TypeScript. Writers and
factories were still unimplemented at that receipt's source. Subsequent work
is qualified separately below.

The [writer unit receipt](validation/wasi-p3-guest-writers-units-20261101.json)
records 125 normal debug-mode component runtime tests and 34 WIT/C tests.
Generated typed factories and writes now implement whole-batch ownership
commit, unconsumed suffix recovery, cancellation and stream closure. Twenty-nine
ASan/UBSan writer cases distinguish consumed, recovered and discarded owners;
negative controls catch consumed-owner drops and illegal unfinished-future
retirement. Actual minor-GC tests verify temporary writer release and severing
of input expando cycles. Guest ABI 2 and endpoint ABI 1 remain unchanged; the
producer operations use a separate additive ABI 1 table. These native tests do
not qualify Wasm writer execution, synchronous factory background scheduling
or public async generation.

The [actual guest-write receipt](validation/wasi-p3-guest-writes-20261101.json)
qualifies the subsequent Linux Wasm profile at source fb38e46cf4. A fresh
coherent ordinary native cohort and both fresh P3 runtime variants pass all
15 acceptance phases, including the full read regression profile. Independent
host readers consume guest-created record, character, void, unit, byte and
nested-future payloads. Async factories return their readers before the
producers finish; host reads then advance the producers without another guest
export call. Partial write suffixes, cancellation/retry, explicit stream close,
nine completed producers and zero producer failures are asserted. The moving
guest copies 12,032 objects with evacuation verification; native host controls
remain off. A separate control against these frozen artifacts confirms the
plain archive rejects instrumentation grants for the expected reason; the
maintained acceptance script now requires that additional control.
Actual resource-bearing endpoint I/O, the wider lifecycle matrix
and private producer scheduling behind synchronous public factories remain.

The [synchronous factory receipt](validation/wasi-p3-sync-factories-20261101.json)
qualifies private producer scheduling for synchronous public root/world
functions on Linux at source a8dad22b7e, after integrating main ee0d9183d4.
All 20 acceptance phases pass, including the earlier guest read/async write
regressions, typed sibling facade composition, plain grant rejection and the
separate maintained-C WAIT/resume control. Independent host reads progress
producers after synchronous factories return, without another guest export.
Eleven producers complete with zero failures and at least 14 partial chunks.
The moving Wasm guest copies 12,870 objects with evacuation verification;
native host GC controls remain off. Thirty-seven WIT/C tests and 125 normal
debug-mode component runtime tests pass, including 30 writer sanitizer cases
and generated facade input-buffer/result-custody checks. Fresh native artifacts
are source matched; P3 archive reuse is justified by complete production-input
Git tree/blob equivalence. Actual resource-bearing Wasm I/O, named-interface
factory interoperation, async imports started from synchronous exports, wider
lifecycle coverage and public adapters remain unqualified.

The [resource endpoint receipt](validation/wasi-p3-resource-endpoints-20261101.json)
qualifies owned exported-resource payloads in actual future/stream I/O and
synchronous named-interface factories at source 373e24f633. All 17 resource
acceptance phases and all 20 earlier endpoint regression phases pass on Linux.
An independent Rust host verifies compound own/optional-own resource payloads,
partial suffix recovery, cancellation/retry, explicit close/discard, stale
handle rejection and 33 exactly-once drops per resource run. No extra guest
export is needed to advance producers after the synchronous factories return.
Resource constructors/methods use typed forwarding components; public type
annotations bind to the original nominal resources. Incoming facade borrows
retire after the forwarded call and before storage cleanup/actor handoff;
a missing-borrow-drop mutation must fail the sanitizer test. Thirty-eight
WIT/C tests pass. The resource moving guest copies 17,356 objects; the full
earlier endpoint moving regression copies 12,870, both with evacuation checks
and native host controls off. The native cohort is freshly source matched;
P3 archive reuse and the separately sourced 125 runtime unit results retain
explicit unchanged-production-input proofs. Imported-resource endpoints,
shared external interface/value type bindings, synchronous-started async
imports, the wider lifecycle matrix and public generation remain open.

The [shared-resource endpoint receipt](validation/wasi-p3-shared-resource-endpoints-20261101.json)
qualifies the subsequent imported-resource and shared-interface profile at
source 239f12b84c. All 19 imported-resource phases, 17 strengthened local-resource
phases and 20 transport regression phases pass. The native Rust provider
constructs resources, validates real borrowed method calls and observes exactly
33 drops per resource run. A second exported interface passes readers and local
resource-bearing records without changing their resource identity. Public
interface types bind through their published dependencies. Moving guests copy
17,811 imported-profile objects, 17,607 local-profile objects and 12,870 transport
objects, with evacuation verified and native host controls off. Forty WIT/C
tests pass; the 125 runtime unit results remain separately sourced and now have
a direct complete production-input identity proof. The fresh ordinary native
cohort and frozen P3 archive sources are recorded separately. All 368 archived
files and their exact regular-file set were verified locally. Repeated labels
implementing the same canonical interface, synchronous-started async imports,
wider lifecycle coverage and public generation remain unqualified.

The [synchronous-started import receipt](validation/wasi-p3-sync-imports-20261101.json)
qualifies the next Linux profile at source 3ec4b6fea8. Synchronous exports stage
async imports for the private actor to start after return. An additive frame
entry rejects incoming call-scoped borrows before committing any custody;
guest-owned loans remain live until native acknowledgement and frame retirement.
The independent Rust host verifies ready and reverse-gated completion, 16
producers, 32 imports, 33 exactly-once resource drops and no pending actor per
mode. Interleaved calls preserve ALS through awaits and moving GC. Both moving
modes have independent evacuation witnesses: 15,041 and 14,527 copied objects.
All 19 new phases and the 19 imported-resource, 17 exported-resource and 20
transport regression phases pass. Forty WIT/C tests, 129 freshly executed
normal-debug component runtime tests and two compiler/API regression tests pass.
The compiler now selects runtime-owned async_hooks without emitting a stdlib
stub during synchronous initialization. Native artifacts and both P3 archives
are freshly built at this source. All 477 indexed evidence files and the exact
regular-file set were verified locally. The earlier async-import fixture still
needs migration from its old synchronous-rejection expectation. Repeated
canonical-interface labels, broader lifecycle/cancellation coverage and public
async generation remain unqualified.

The [upstream WebSocket acceptance receipt](validation/websocket-aot-20261101.json)
qualifies native, P1 and P3 output for the unchanged external TypeScript server
on Linux at source `02cf3068ae`: 27 phases include exact control/boundary/20 MiB
wire cases and both unchanged real socket clients on every output format. P1
imports only standard Preview 1 interfaces; P3 declares exact 0.3.0 command
imports. P2/P3 fs entries now own descriptor duplicates, keeping libc's instance
stdio alive across task teardown. The P3 host's parallel compiler meets the
text client's original four-second deadline without Tokio. Eleven compiled P3
commands and 45 ordinary command regression phases pass. The native cohort is
fresh; guest runtimes retain their `e48d556ab6` source plus selected-production
identity proof. Runtime units at that source include 129 component tests and
the descriptor teardown test; they are not newly executed at the native source.
At the newer source, 41 WIT/C tests, 23 upstream type tests, 32 upstream graph
cases and 11 normal-debug command host tests pass. Composition now preserves
separate nominal resources for repeated labeled instances of one canonical
interface and rejects the wrong owner. Actual generated repeated-label endpoint
interop, the wider lifecycle matrix and public async generation remain open.
All 505 indexed evidence files and the exact regular-file set were verified
locally. Release performance, broader providers, current instrumented native
artifacts, installed platform packages and qualified landing are not established.

The [P3 compound and cancellation receipt](validation/wasi-p3-compound-cancellation-20261101.json)
qualifies the migrated compound import/export fixtures at source `b072f996f5`.
The actual P3 facade profile passes 24 import phases, 18 export phases and an
eight-phase canonical cancellation parent, whose first phase runs those import
checks. All 75 synchronous-started import and resource/transport regression
phases also pass. Synchronous `guarded` imports now stage successfully rather
than expecting the obsolete rejection. A maintained-C caller cancels two native
children in each of eight rounds while preserving a live sibling and reusing
the Store. Moving guests copy 37,766 objects in the import profile, 30,921 in
the export profile and 8,031 in canonical cancellation, with evacuation verified
and native controls off. Ready import futures have a separate moving witness;
they do not prove canonical inline completion. Both plain and instrumented
guest archives retain their original source identities and exact selected-input
reuse proofs. The native cohort is freshly built; runtime unit tests were not
reexecuted by this run. All 681 indexed evidence files and their exact regular
file set were verified locally. Public generation, returned-actor lifecycle
auditing, actual repeated-label endpoint execution and the remaining rows stay
open. Subsequent private-name collision fixes have separate unit qualification.

The [weak-slot repair receipt](validation/wasi-moving-weak-slots-20261102.json)
qualifies a later runtime fix at `5cd1705734`. Fresh release WebSocket output
passed at `6ea71cc1a3`, but its separate shared-resource moving test exposed a
stale FinalizationRegistry target. Copying now repairs remembered weak slots
without tracing through them and retains their custody for later minors.
The same new regression fails on the previous runtime and passes after two
actual moves followed by weak-only target collection. All 6,422 runtime and
299 stdlib tests pass together with normal debug assertions and serial execution.
A fresh instrumented P3 archive passes all 17 shared-resource and 20 transport
phases, relocating 17,789 and 12,898 objects with evacuation verification.
The native compiler and plain reference archive are reused at `6ea71cc1a3`;
this is targeted runtime qualification. All 138 indexed evidence files and
their exact sizes, hashes and regular-file set were verified locally.

The [balanced production cohort receipt](validation/wasi-balanced-production-cohort-20261102.json)
qualifies source `3a20cda3b5` with a fresh coherent native `prod` union and
P1/plain-P3/instrumented-P3 archives built through the size-balanced builder.
All eleven derived compile commands were reexecuted and pass, along with the
strict Tokio audit, 44 WIT/C tests (two ignored), 222 component-host tests and
four docs generator regressions. The subsequent extern-name projection
requires separate qualification.
All 27 WebSocket phases, including eight real client cases, pass. Compound
imports/exports, canonical cancellation, synchronous-started imports and the
resource/transport regressions pass at the same artifact source. Moving
witnesses include 37,766 imported-profile objects, 30,921 exported-profile
objects, 29,568 synchronous-started objects, 17,963 imported-resource objects,
17,789 shared-resource objects and 12,898 transport objects, with evacuation
verification. The 6,422 runtime and 299 stdlib unit results retain their
separately executed `5cd1705734` source and complete production-input equality
proof. All 486 indexed evidence files, their sizes, hashes and exact regular
file set were verified locally. New repeated-label and returned-actor profiles,
public tooling, providers, installed P3 packages and qualified landing remain.

| Requirement | Required evidence | Current state |
| --- | --- | --- |
| Canonical async imports and exports | Public generated components compose in both directions; overlapping calls, immediate/delayed completion, errors, post-return work and Store reuse | Experimental scalar/compound and recorded synchronous-started import profiles qualified on Linux; public CLI guarded |
| Resource ownership across async calls | Imported/exported nominal resources, compound aliases, pre-start cancellation, returned-result races, borrow expiry at return/cancel, exactly-once drops and moving GC | Actual experimental entry point qualifies basic and compound C/Perry/C profiles on Linux, including 24 duplicate-owner rejections, 188 exactly-once drops, unclaimed results and moving GC; public async CLI remains guarded |
| Guest `future<T>` and `stream<T>` | Readable/writable ownership, arbitrary supported WIT payloads, void, bounded writes, backpressure, final partial chunks, closure, cancellation and cleanup across suspension | Experimental reads/writes and sync/async factories qualified on Linux, including owned exported-resource records through named-interface factories: latest receipt has 17 resource phases plus 20 earlier-profile regressions, 17,356 and 12,870 relocated guest objects respectively, and 38 WIT/C tests. Runtime125 unit evidence is separately sourced with unchanged production inputs; 30 writer and 26 reader sanitizer cases pass. Subsequent imported/shared-resource receipt qualifies 19 imported phases, 17 strengthened local-resource phases and 20 transport regressions with 40 WIT/C tests. The subsequent synchronous-started import receipt qualifies 19 new phases plus all 56 earlier regressions at the new runtime, with 40 WIT/C and 129 fresh runtime unit tests. Repeated-label composition and wrong-owner rejection now pass unit validation; actual repeated-label endpoint interop, wider lifecycle coverage, staged-profile canonical cancellation and public adapters remain |
| WASI 0.3.0 command output | A public command/runtime profile with `wasi:cli/run@0.3.0`, explicit system-interface versions and execution in a host providing the declared 0.3.0 imports; no implicit reliance on P2 compatibility | Integration branch implements `--target wasip3`. The fresh Linux prod/release WebSocket receipt at `6ea71cc1a3` qualifies 27 functional phases including eight real socket client cases. The earlier `02cf3068ae` receipt separately qualifies eleven compiled P3 commands and 45 command phases. Full current command regressions, installed platform qualification and landing remain; `--target wasi` retains P2 |
| Complete supported 0.3.0 WASI interfaces | Commands, clocks, randomness, filesystem, TCP/UDP/DNS and HTTP execute through Turnloop with capability checks, I/O cancellation and shutdown | Linux provider/command/HTTP receipts exist; complete current-profile matrix still required |
| Cross-language interoperability | Public Perry output composed with independent Rust and maintained-C components, including resources and future/stream payloads | Scalar/compound cancellation examples and current actual resource-bearing endpoint/named-interface factory profiles exist; subsequent shared-resource receipt qualifies imported resources and cross-export record/future aliases on Linux; repeated-label nominal composition passes unit validation; actual generated repeated-label endpoint interop and the wider value matrix remain unqualified |
| Runtime integration | Promise/nextTick task isolation, rejection identity, ALS, suspended-value tracing, teardown and positive moving-GC witnesses | Public ALS constructors/methods link from runtime alone; recorded Linux imports/exports and moving-GC profiles pass. Actual P3 libraries now preserve agent TLS across SDK tasks, including canonical WAIT/resume. The recorded synchronous-started import profile also qualifies ALS across interleaved native loans/exchanges and both moving modes. Shared-provider regression and wider platform/package coverage remain |
| HTTP layouts | Current supported layouts and limitations explicitly tested; concurrent root handlers, nested/imported fallback, streaming bodies and cancellation | Linux supported layouts qualified; broader composition coverage open |
| Platform and packaging | Current source-matched ordinary/instrumented artifacts and installed public tools run on Linux x86_64, macOS ARM64 and Windows x64 | Current Turnloop execution qualified on Linux; Windows compile only |
| Durable regression coverage | Required CI executes the advertised profiles and fails if the actual guest, cancellation or GC witness is absent | Existing gates cover implemented profiles; extend for completed profiles |
| Dependency and compatibility invariants | No Tokio anywhere, existing P1/P2 profiles retained, coherent compiler/runtime/adapter ABI, no workspace version bump | Strict Tokio ban merged; verify every dependency and landing change |

The finish line is every row verified on the current implementation, followed
by updating the public support contract and landing the qualified work. Public
guards are removed only as their complete replacement profile is verified.
WASI 0.3.1 maps/annotations and experimental `error-context` are outside this
0.3.0 objective, according to the [official adoption table](https://wasi.dev/releases#component-model-features).

The first integration changes address two canonical resource rules: commit
lowered export owners before returning them, and retire incoming borrows before
`task.return`/`task.cancel` while keeping post-return task values rooted. The
maintained C backend's automatic borrow dropping applies to synchronous C stack
lifetimes; async adapters must instead own those handles through suspension.
See the [Canonical ABI](https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md).
