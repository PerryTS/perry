# RegExp program ownership and caching

JavaScript RegExp construction uses Perex directly. There is one engine;
lookbehind, backreferences, and other accepted patterns share the same
construction path. `perex_construct.rs` retains eager syntax checking, so
invalid patterns still throw at construction and the first search does not
inherit deferred compilation work.

`regex/perex_cache.rs` retains immutable programs by source plus canonical
flags. The first lookup uses the source `StringHeader` identity and reads no
pattern bytes. An identity miss hashes the original bytes, then verifies exact
equality within the hash bucket. Independently allocated equal strings share
a program. Flags remain part of both keys; canonical ordering makes `ig` and
`gi` equivalent. Every new RegExp still owns its original source/flags and
independent `lastIndex`. Retained source strings are marked shared to prevent
unique-string append from modifying the cached pattern or OriginalSource.

The cache holds at most 512 entries and 32 MiB of source/program payload.
Least-recently-used entries are evicted individually. Programs above the byte
limit remain usable through their RegExp owner without cache retention.
Each cache registers its scanner before publishing its first root, so unused
caches add no GC scanner work. Source and program slots are mutable GC roots;
evacuation rewrites them and
rebuilds the source-identity index. Content hashes are address independent.

`regex/perex_binding_cache.rs` similarly bounds validated Perex bindings.
`BoundProgram` retains its original immutable owner rather than validating a
fresh program view for every search. That owner's address lives in an
`Rc<Cell<*const u8>>`. A mutable-root scanner visits a list of weak references,
so an operation's cloned binding survives eviction and collection, while a
weak registration cannot retain an unused owner. Movement rebuilds the cache's
address index. No borrowed program slice survives a safepoint. The cache keeps
only small scalar scratch-size hints; scratch buffers remain operation-owned
and charged to the existing memory budget.

The fast builtin dispatch guard contains only immutable ShapeIds, field
indices, and epochs, with no untraced GC address. It verifies the current
builtin `exec` value, the receiver's metadata, expando table, and prototype.
Empty-string replacement additionally verifies builtin flags getters and
`Symbol.replace`, and requires primitive strings and numeric `lastIndex`.
Observable overrides take the ordinary dispatch path. Admitted replacements
use full-subject UTF-16 spans and preserve global/sticky index updates and
Unicode empty-match advancement, without allocating exec result arrays.

`PERRY_REGEX_DIAG=1` reports compile and validation counts, identity/content
hits, bytes hashed, canonical dispatches, searches, and scratch allocation and
growth counts. Diagnostics deliberately inspect source bytes for attribution;
measure timings with diagnostics disabled. Cache payload lives in traced GC
cells; the census separately reports native cache metadata as
`regex.program_cache` and `regex.program_bindings`.
