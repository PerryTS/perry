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
The cache registers its scanner before publishing its first root, so programs
that never construct a RegExp add no cache scanner work. Source and program slots are mutable GC roots;
evacuation rewrites them and
rebuilds the source-identity index. Content hashes are address independent.

Perex's `ProgramWitness` lives beside the immutable words in each program GC
cell (upstream #10166/#10183). The first binding validates the words; subsequent
bindings use that witness in constant work, including bindings from another
RegExp sharing the cached program. Each operation still roots its own program,
so eviction, collection, and reentrant receiver recompilation cannot invalidate
an active search. No borrowed program slice survives a safepoint. Scratch
buffers remain operation-owned and charged to the existing memory budget.

Compound split/replace/match operations retain program and subject bindings in
`perex_api::Reuse`, checking the current receiver and program cell before reuse.
Within a compound operation, the previous position is reused only with the same
subject binding. Across JavaScript calls, upstream's four-entry scalar position
table can reuse a non-ASCII string's cursor when its concealed address, lengths
and heap generation still agree. Heap changes or in-place length changes
invalidate that identity. The table does not retain a string or program.
Validated heap strings use the existing counted-subject path.

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
growth counts. Set it while compiling as well as while running: it is
served by perry-runtime's `hot-diag` feature, which an auto-optimized build only
links when the knob (or `PERRY_GC_INSTRUMENTS=1`) is set at compile time, and a
binary built without it aborts at startup rather than report nothing. Diagnostics deliberately inspect source bytes for attribution;
measure timings with diagnostics disabled. Cache payload lives in traced GC
cells; the census separately reports native cache metadata as
`regex.program_cache`.
