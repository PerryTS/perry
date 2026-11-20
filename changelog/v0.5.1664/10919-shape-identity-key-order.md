Added parity coverage pinning property **key order** across every way an object
reaches a layout (#10868 stage 0).

Key order is observable through `Object.keys`, `JSON.stringify`, `for…in` and
spread, and Perry reaches a given layout by several different routes — literal,
incremental assignment, `Object.assign`, spread, delete-and-re-add, integer-like
keys, and the transitions between them. Nothing pinned that the routes agree
with each other *or* with the specification's insertion/integer-index ordering,
so a layout change could silently reorder one route only.

Test-only: a parity fixture that drives each route to the same key set and
compares the observed order byte-for-byte against the Node oracle. It is stage 0
of #10868 — it locks current behaviour in place before the layout work that
follows can move it.
