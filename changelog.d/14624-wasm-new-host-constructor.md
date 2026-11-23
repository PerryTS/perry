`--target web`: `new Map()` (and `new Set(...)`, or any host constructor reached
without a dedicated lowering) aborted the page with `TypeError: Constructor Map
requires 'new'`, because a run-time `new` called the constructor as a plain
function. It is now constructed; a Perry closure keeps the plain call. A
run-time `new` with more than three arguments no longer yields `undefined`.
