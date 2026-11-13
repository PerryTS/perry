`raw_ptr_from_value` and its copies (the value decoders of the node:stream,
node:events, dgram, util.diff, stream/consumers, stream/promises, trace_events,
node:test, repl and inspector entries, the Map / Set / Promise / Array subclass
constructors, `fs.writeFile` data and `TextDecoder.decode`) decode by the tag
alone. Each had an arm "a word below 2^48 is an address", so an address-shaped
Number (a denormal double) handed as a listener, a chunk, a stream, a callback
or a buffer source was dereferenced: `emitter.on("x", e)`, `once`,
`addListener`, `prependListener`, `off`, `readable.on("data", e)`,
`writable.write(e)`, `stream.pipeline(e, cb)`, `util.diff(e, "a")`,
`new TextDecoder().decode(e)`, `fs.writeFileSync(path, e)`, `socket.send(e)`
and 24 more operations crashed with SIGSEGV for `e = Number.MIN_VALUE * 2 ** 28`
(node: ERR_INVALID_ARG_TYPE). 15 decoders in 15 files lose the arm; such a
Number is now treated like every other Number there. No switch: the previous
decode is the memory-unsafe one.
