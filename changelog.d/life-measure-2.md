### Fixed
- Lifetime measurement: standalone constructors, record `$sig` returns and the rejection handler of direct async bodies now end their activation through the return/catch hooks (they were popped late by the next hook). The entry hook carries the body's id and the run writes per-body exit counts (`#body` rows: entered, returned, popped at a handler, by a shallower frame, at turn end).
