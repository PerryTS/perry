The family prototypes the realm seals at startup (String, Number, Boolean, Symbol,
BigInt, Map, Set, Date, RegExp, Promise, Error, WeakMap, WeakSet) are born on the
reserved prototype shape lineage too, so sealing them mints no fresh shape.
Startup runs fewer instructions; locked-prototype semantics are unchanged.
