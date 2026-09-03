**`PERRY_GC_CENSUS=<path>` — an env-gated heap census that says what the
live set is made of.** Off by default; when set, every explicit `gc()` and
every `SIGUSR2` (serviced by the event loop on the main thread) runs a full
collection and, at the point where marking is complete and nothing has been
swept yet — the only point where live/dead is exact for every generation —
walks every arena block and every malloc'd GC object and appends one JSON
line to `<path>`: live/dead bytes per space, per GC type, per class
(inline-slot capacity vs used, meta records), string/array/closure payload
vs header bytes, a NaN-box tag histogram of every live slot, the block-level
capacity/used/free-list/hole/pool accounting that separates "live" from
"free but dirty", and the entries+estimated bytes of every runtime side
table outside the GC heap (function name/source registries, closure
registries, shape table, class registries, stack-map index, intern table,
remembered sets, page metadata, timers, module paths). Process RSS,
`phys_footprint` (macOS) and mimalloc's commit statistics ride along so the
walk can be reconciled against what the OS charges.

Validated against a heap of known composition (`gc::tests::census`): 1000
rooted 100-byte strings and 500 rooted 8-slot objects appear with exactly
those counts and header-inclusive sizes, and reappear as *dead* once their
roots are dropped. When the variable is unset nothing is installed or armed;
the residual cost is one relaxed atomic load per event-loop wait.
