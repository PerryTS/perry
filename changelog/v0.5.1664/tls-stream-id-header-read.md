Fix a SIGSEGV when a Web Stream reached the handle property dispatcher with
`node:net`/`node:tls` linked (`"x" in readableStream`, as Effect's
`Predicate.hasProperty` does; it killed the OpenCode TUI on every run). The TLS
arm re-boxed the numeric stream id as a heap pointer and read a GC header at
`0x100000 - 8`. Every header gate in `addr_class` (`is_plausible_heap_addr`,
`try_read_gc_header`, the tracked reader) now rejects the Web Streams id band
as well as the handle band; native payload receivers are resolved with the
ownership-proving reader; and handle dispatch converts its `i64` receiver back
to a value through one runtime helper that keeps a stream id numeric.
