### Added
- Lifetime measurement: a dynamic iteration boundary (`dyn`, `dyn2`): the end of the iteration of the nearest loop in progress anywhere on the stack at the birth, and of the one around it (`perry_life_loop` where a loop body iteration starts). Fixtures m8 (callee birth dead at the caller's iteration end) and m9 (callee birth kept by an accumulator outside the loop); the lifekeep control runs on m2 and m8.

### Fixed
- Lifetime measurement: standalone constructors, record `$sig` returns and the rejection handler of direct async bodies now end their activation through the return/catch hooks (they were popped late by the next hook). The entry hook carries the body's id and the run writes per-body exit counts (`#body` rows: entered, returned, popped at a handler, by a shallower frame, at turn end).
