Generator loop conditions now preserve values passed to `.next(value)`.

The generator linearizer creates temporary locals when a `yield` appears inside
a loop condition. Those locals were already preallocated and captured across
resume states, but their state-local declarations still created shadow boxes.
The resumed value was written to the shadow while the condition read the
untouched captured box, so `while ((value = yield n) !== "stop")` observed
`undefined`, completed early, or accumulated the wrong values.

Linearizer-created locals now use the same declaration-to-assignment rewrite as
ordinary hoisted generator bindings, keeping every state on the preallocated
capture. The eight-case #5933 loop-condition suite passes byte-for-byte against
Node, including repeated yields and a terminating `.next("stop")`.
