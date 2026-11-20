`for await` over a zlib transform stream (`createGunzip()`, or a
`readable.pipe(createGunzip())` result) now yields the output. zlib streams
are handles, not objects, so they had no `Symbol.asyncIterator`; the runtime
now builds the node:stream iterator for them, attached through their own
`on`/`off` (which zlib streams now support).
