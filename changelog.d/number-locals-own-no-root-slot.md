A Number local owns no GC root slot in any body kind: methods, static methods
and closures now drop the shadow slot of every member of the function's
Number-local set, as functions and module init already did, so its stores pay
no slot bind, lexical-death clear or incremental root shading (32 closures and
42 slots in the tsc bundle, mostly `flags |= f(x)` accumulators). The inline
number guard in front of raw-double arithmetic now ignores the sign bit, so a
negative NaN whose `fneg` image is a pointer tag can no longer reach a Number
local through an admitted operand (charter step 5L, P6).
