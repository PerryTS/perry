Built-in Node constructor values inherit their immediate parent constructor and
parent prototype through their actual shapes, including internal intermediate
classes. Static reads use the ordinary property path. EventEmitter's shared
`defaultMaxListeners` accessor follows that chain. Saved net brand predicates,
SocketAddress.parse, ECDH.convertKey and ReadableStream.from now use the existing
native implementations. Constructor materialization publishes its rooted value
before resolving circular child exports, preserving identity. Worker constructor decoration is armed by its import, preserving module dead stripping.
