Fetch now resolves when the final response headers arrive and delivers decoded body chunks through its ReadableStream while the download continues. Whole-body methods consume that stream asynchronously; clone, tee and iteration use the same queue. Reader cancellation, body cancellation and abort after headers close the owning agent's exchange, and later transport or decoding failures reject body consumers.

Native reads rearm only while the consumer has byte credit. Content decoders yield bounded output blocks, including high-expansion compressed input. Followed redirects abandon their unread bodies before starting the next hop. Incremental chunks and pending consumers use the existing stream GC scanner, with no forced collection in the request path.

Internal stream promises now carry their handled-rejection state in the Promise header, preserving it through copying GC and removing an address-keyed membership set. Pending reader batches and temporary delivery values remain rooted through allocating callbacks.

Readable.from drives live iterators one result at a time and yields to the agent loop while next() is pending. Pipes subscribe to native destinations through their own emitter, so a gzip write that pauses the source resumes on drain instead of hanging a streamed worker unpack.
