**One shared table for the CommonJS-default module set; claude-code's MCP
debug logger shape and exec/execFile callback order pinned (#9500).**

The set of Node builtins whose `require()` / default import hands out a
distinct `<mod>.default` namespace was hand-maintained in five places (two of
them inside the HIR alone, already disagreeing on `ffi`, `inspector`,
`inspector/promises` and `wasi`); the method-call router's copy is the one
that drifted far enough to break `require('child_process').spawn` (#9485,
#9498). The table now lives once in `perry-dispatch`, built from one literal
per module, and the runtime's property-read and method-call paths, the
`default`-export resolver and the HIR's import lowering all derive from it.
Adding a module is one line; tests pin the table's shape, the HIR's
classification of every row, and the router test's list against the table in
both directions. No behaviour change.

Two fixtures pin the issue's other findings. The MCP debug logger's exact
write shape — the `using`-downlevel fs wrapper, the timer/dispose buffered
writer, the graceful-shutdown cleanup set and the `appendFileSync` → ENOENT →
`mkdirSync(recursive)` recovery arm that is the only code creating the log
tree — is byte-compared to node; it fails on a pre-#9491 build (the append
did not throw, so the tree was never created) and passes on main. The
exec/execFile callback order is pinned as what node guarantees — completion
order, whichever API launched the child or came first; the inverted order for
two instant `echo`s is a same-turn batch-delivery artefact node flips with
submission order, not a rule.
