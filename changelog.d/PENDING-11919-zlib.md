### Changed

- Use runtime Transform state for all eleven `node:zlib` constructors. Codec payloads use StreamHooks, deferred steps, bounded output and external byte accounting; destroy releases the codec immediately.
- Queue one-shot callbacks as traced runtime closures and return plain values from synchronous codecs. Read and create bytes through the Buffer B1 API.
- Implement lazy stream state initialization and shared payload allocation, attachment and prototype operations for binding crates.

### Removed

- Remove the bundled stdlib zlib provider, its stream tables and method/property dispatch arms, and the binding's private queues, listener maps, root scanner and agent table lifecycle. Default and optimized builds select `perry-ext-zlib`.

Validation and performance acceptance are pending while this lane is under development.
