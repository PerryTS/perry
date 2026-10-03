`node:stream` keeps a stream and its chunks valid when a collection moves
them. Hidden-field keys and event names no longer allocate: each literal is
one longlived string per thread. The readable drain, flush, end, resume and
pipe-write paths and `Readable.from` hold the stream and chunks in handles
across listeners, pipe writes and the source iterator. Before, a collection
there left them at retired addresses: `Readable.from(gen).pipe(createGunzip())`
read with `for await` could hang with an unsettled top-level await or crash.
