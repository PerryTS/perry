### Preserve streamed responses across module boundaries

`Response` subclasses such as Next.js's `NextResponse` now keep their native
response identity, live header and cookie mutations, and `ReadableStream`
bodies when returned synchronously or through promises from another module.
Shared-runtime dylib builds also register stream roots with the runtime
provider and index GC maps from loaded app images, so moving collections keep
queued stream state alive through a full drain.
