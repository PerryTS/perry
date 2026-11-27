ReadableStream, its readers and controllers are ordinary GC objects; the stream registries are gone (fixes a ReadableStream retention leak); pipeTo reads its options in node's order
