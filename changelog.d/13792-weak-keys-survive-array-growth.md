WeakMap, WeakSet, Map, Object.is and FinalizationRegistry.unregister find an
array under both addresses growth gives it: the forwarding stub a table or an
alias kept, and the new head the growing binding holds. Identity is the
forwarding-resolved head, as === defines it. A lookup that hits on bits pays
nothing for this; only a miss of an array key asks: a small Map scans for stub
keys only when an array grew since it last proved it holds none, a large Map
and the weak tables find the array by the relocation-stable identity token Set
already uses.

A collected FinalizationRegistry cell stays registered until its cleanup job
runs: gc() no longer delivers callbacks at its own tail, so
`gc(); registry.unregister(token)` returns true and cancels the callback, as
in node.
