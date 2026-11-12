### Added
- Lifetime measurement: a dynamic iteration boundary (`dyn`, `dyn2`): the end of the iteration of the nearest loop in progress anywhere on the stack at the birth, and of the one around it (`perry_life_loop` where a loop body iteration starts). Fixtures m8 (callee birth dead at the caller's iteration end) and m9 (callee birth kept by an accumulator outside the loop); the lifekeep control runs on m2 and m8.

- Lifetime measurement, allocation census (instrumented runtime, `PERRY_LIFE_ALLOC=<path>` at run time): every allocation the collector serves counted exactly by object type on the runtime path and on the compiled inline bump path (headers of each inline burst walked at write-back), covered bytes counted at the census birth hook; byte-proportional samples (`PERRY_LIFE_ALLOC_SAMPLE`, default 16 KiB) keep raw return addresses for offline symbolization and whether a birth claimed the object.

### Fixed
- Lifetime measurement: the dynamic boundaries are decided by the same forced minor as the iteration/return probe at that point (a second minor promoted the survivors the other probe then saw old and left unresolved).
- Lifetime measurement: standalone constructors, record `$sig` returns and the rejection handler of direct async bodies now end their activation through the return/catch hooks (they were popped late by the next hook). The entry hook carries the body's id and the run writes per-body exit counts (`#body` rows: entered, returned, popped at a handler, by a shallower frame, at turn end).
