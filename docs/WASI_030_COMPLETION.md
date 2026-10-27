# WASI 0.3.0 completion

Objective: complete supported WASI 0.3.0 hosting and Perry-authored component
output, including reusable typed plugin interfaces. The current baseline is
main `315930289939d39cb0cf0c876c03e82da6000d3a`. Existing historical receipts
retain their original scope. An implemented helper or passing narrow test does
not establish completion of a row below.

| Requirement | Required evidence | Current state |
| --- | --- | --- |
| Canonical async imports and exports | Public generated components compose in both directions; overlapping calls, immediate/delayed completion, errors, post-return work and Store reuse | Experimental scalar/compound profile qualified; public CLI guarded |
| Resource ownership across async calls | Imported/exported nominal resources, compound aliases, pre-start cancellation, returned-result races, borrow expiry at return/cancel, exactly-once drops and moving GC | Runtime loan foundations exist; adapter integration in progress |
| Guest `future<T>` and `stream<T>` | Readable/writable ownership, arbitrary supported WIT payloads, void, bounded writes, backpressure, final partial chunks, closure, cancellation and cleanup across suspension | Native hosts have readers/producers; guest adapters guarded |
| Complete supported 0.3.0 WASI interfaces | Commands, clocks, randomness, filesystem, TCP/UDP/DNS and HTTP execute through Turnloop with capability checks, I/O cancellation and shutdown | Linux provider/command/HTTP receipts exist; complete current-profile matrix still required |
| Cross-language interoperability | Public Perry output composed with independent Rust and maintained-C components, including resources and future/stream payloads | Scalar/compound and cancellation examples exist; missing value profiles unqualified |
| Runtime integration | Promise/nextTick task isolation, rejection identity, ALS, suspended-value tracing, teardown and positive moving-GC witnesses | Core task/GC profile qualified; guest ALS constructor packaging incomplete |
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
