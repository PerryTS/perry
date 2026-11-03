**Nine `Array.prototype` higher-order methods kept the raw callback pointer
they were handed and re-used it after the callback had allocated** — a
collection landing inside the loop retired the closure, and the next dispatch
read recycled memory. When that memory had been reissued as an ordinary object
the validator reported its `typeof` instead: `TypeError: object is not a
function`. That is the error claude-code's OAuth login died with (#9673).

A callback born at the call site — the inline arrow in `xs.forEach(x => …)` —
is reachable only through the raw parameter and the native stack, and an
evacuating minor does not scan the native stack. `js_array_map` learned this in
#6081/#6206, `js_array_filter` alongside it, and `js_array_map_discard` again in
#7533 when an unrelated allocation change moved a collection into its window.
The remaining arms — `forEach`, `some`, `every`, `find`, `findIndex`,
`findLast`, `findLastIndex`, `flatMap` and `reduce` — were never converted and
still bound the pre-collection address. Each now roots the callback for the loop
and re-reads it at every dispatch, NaN-boxed so the read-back stays out of
`scripts/raw_handle_debt.py`'s ledger (the shape `map_discard` already used).

The defect was latent in exactly the way the earlier three were: a stale
address only bites when a collection lands in its window, and nothing put one
there in the suites. `PERRY_GC_PROTECT_FROMSPACE=1` — which mprotects retired
from-space so a stale use faults at the instruction that makes it — puts the
question beyond timing. On unfixed `main`,
`test-files/test_gap_9673_array_callback_rooting.ts forEach` dies there with
`obj_type=4 size=32` (a `GC_TYPE_CLOSURE`) and a backtrace naming
`js_array_forEach + 4072`; with the fix the whole fixture runs, byte-identical
to `node --experimental-strip-types`, with and without the instrument.

Three separate one-arm fixes across three issues is a pattern, so the invariant
is now checked rather than remembered: `array/callback_rooting_tests.rs` reads
this module's own source and fails if any arm dispatches the raw parameter, or
resolves a direct-call site for a callback it never roots. A new arm has to
root, or the test names it.
