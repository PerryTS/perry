Fix stale chunk and receiver addresses in the direct `node:stream/promises`
pipeline when a destination method or method getter triggers moving GC.
