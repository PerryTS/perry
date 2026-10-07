### Changed

- Use runtime Transform state for all eleven `node:zlib` constructors. Codec payloads use StreamHooks, deferred steps, bounded output and external byte accounting; destroy releases the codec immediately.
- Queue one-shot callbacks as traced runtime closures and return plain values from synchronous codecs. Read and create bytes through the Buffer B1 API.
- Implement lazy stream state initialization and shared payload allocation, attachment and prototype operations for binding crates. Keep synchronous Readable.from iterators lazy and bounded by readable credit, with their source traced across collection.
- Match Node's gzip bytes across multiple writes with the bundled zlib encoder and count its allocations through its allocator hooks. Preserve the platform gzip OS byte.
- Route inherited codec methods through the ordinary prototype chain, honor subclass transforms and pipe write overrides, and complete deferred writable work before async iterator continuations.

### Removed

- Remove the bundled stdlib zlib provider, its stream tables and method/property dispatch arms, and the binding's private queues, listener maps, root scanner and agent table lifecycle. Default and optimized builds select `perry-ext-zlib`.

Validation against the last buildable main, 78e2ab97e7: zero new failures across runtime, stdlib, codegen and ext-zlib; 54/58 selected gap cases pass versus 38/58 on main, with zero regressions. All ten real-program outputs agree with Node, including the previously failing buffer_heavy gunzip pipe. The historical n=5 interleaved performance comparison has two small explained increases: hello +0.1236% from linked pointer relocations and commander +0.0503% from the generic lazy-state guard on emitter methods. Other instruction deltas are within their same-binary floors. Named fix-forward plans and RSS/full-collection evidence are in docs/native-payload-zlib-validation.md. Native handle tables/producers fall from 197/183 to 195/161; the Buffer layout gate has zero new sites. All 25 stream/codec/lazy/source/worker sabotages and the nine Buffer layout sabotages turn red. The literal total-process bomb RSS bound remains unmet; queue and codec workspace stay bounded, with clean-process RSS reported explicitly.

Final verification block: the lane is rebased onto origin/main 31bbc3da48.
Both that main and the rebased lane fail the requested release build and
four-crate test command with the same 15 SWC parser errors after nine upstream
dependency updates. The ext-zlib release library check passes with Brotli 9.
Current-main gap/oracle/performance reruns remain blocked; the earlier table
is historical evidence, not acceptance for the new main. See the validation
record for paired error receipts and the required rerun after upstream repair.
