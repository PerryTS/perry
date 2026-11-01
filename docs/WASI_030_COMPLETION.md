# WASI 0.3.0 completion

Objective: complete supported WASI 0.3.0 hosting and Perry-authored component
output, including reusable typed plugin interfaces. The implementation branch
is integrated with main `f163fcb4157b83ff974f8378d68739c343e90c2c` as of November 1.
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

| Requirement | Required evidence | Current state |
| --- | --- | --- |
| Canonical async imports and exports | Public generated components compose in both directions; overlapping calls, immediate/delayed completion, errors, post-return work and Store reuse | Experimental scalar/compound profile qualified; public CLI guarded |
| Resource ownership across async calls | Imported/exported nominal resources, compound aliases, pre-start cancellation, returned-result races, borrow expiry at return/cancel, exactly-once drops and moving GC | Actual experimental entry point qualifies basic and compound C/Perry/C profiles on Linux, including 24 duplicate-owner rejections, 188 exactly-once drops, unclaimed results and moving GC; public async CLI remains guarded |
| Guest `future<T>` and `stream<T>` | Readable/writable ownership, arbitrary supported WIT payloads, void, bounded writes, backpressure, final partial chunks, closure, cancellation and cleanup across suspension | Experimental reads, writes, factories behind async exports and post-return producers qualified on Linux: 15 actual acceptance phases, 12,032 relocated guest objects, 125 component runtime tests, 34 WIT/C tests and 29 writer sanitizer cases (plus 26 read cases). Actual resource-bearing I/O, the wider lifecycle matrix, synchronous factory producer actor and public adapters remain |
| WASI 0.3.0 command output | A public command/runtime profile with `wasi:cli/run@0.3.0`, explicit system-interface versions and execution in a host providing the declared 0.3.0 imports; no implicit reliance on P2 compatibility | Integration branch implements `--target wasip3`; nine Perry commands and 51 Linux acceptance phases pass, plus ten separate Wasm-guest GC phases; current runtime changes, native production variants, release/platform qualification and landing remain; `--target wasi` retains P2 |
| Complete supported 0.3.0 WASI interfaces | Commands, clocks, randomness, filesystem, TCP/UDP/DNS and HTTP execute through Turnloop with capability checks, I/O cancellation and shutdown | Linux provider/command/HTTP receipts exist; complete current-profile matrix still required |
| Cross-language interoperability | Public Perry output composed with independent Rust and maintained-C components, including resources and future/stream payloads | Scalar/compound and cancellation examples exist; missing value profiles unqualified |
| Runtime integration | Promise/nextTick task isolation, rejection identity, ALS, suspended-value tracing, teardown and positive moving-GC witnesses | Public ALS constructors/methods link from runtime alone; recorded Linux imports/exports and moving-GC profiles pass. Actual P3 libraries now preserve agent TLS across SDK tasks, including canonical WAIT/resume. Shared-provider regression and wider platform/package coverage remain |
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
