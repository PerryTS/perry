### Changed

- Use runtime Transform state for all eleven `node:zlib` constructors. Codec payloads use StreamHooks, deferred steps, bounded output and external byte accounting; destroy releases the codec immediately.
- Queue one-shot callbacks as traced runtime closures and return plain values from synchronous codecs. Read and create bytes through the Buffer B1 API.
- Implement lazy stream state initialization and shared payload allocation, attachment and prototype operations for binding crates. Keep synchronous Readable.from iterators lazy and bounded by readable credit, with their source traced across collection.
- Match Node's gzip bytes across multiple writes with the bundled zlib encoder and count its allocations through its allocator hooks. Preserve the platform gzip OS byte.
- Route inherited codec methods through the ordinary prototype chain, honor subclass transforms and pipe write overrides, and complete deferred writable work before async iterator continuations.

### Removed

- Remove the bundled stdlib zlib provider, its stream tables and method/property dispatch arms, and the binding's private queues, listener maps, root scanner and agent table lifecycle. Default and optimized builds select `perry-ext-zlib`.

Functional validation: zero new failures across runtime, stdlib, codegen and ext-zlib; 54/58 selected gap cases pass versus 38/58 on main, with zero regressions. All ten real-program outputs agree with Node, including the previously failing buffer_heavy gunzip pipe. Performance acceptance and delta attribution are still running; see docs/native-payload-zlib-validation.md.
