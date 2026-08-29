The `for…of` desugar's per-element `IteratorNext` is now one runtime call.

The generic desugar performed, per element, a dynamic `.next()` dispatch, a
separate validation call, an override probe that allocated a `"next"` key string
and ran a by-name prototype lookup, and — for builtin collection iterators — a
fresh `{value, done}` allocation. V8 escape-analyses all of that away; perry
executed it literally, and the two collection-iterator dispatchers were 8.6% of
an ECS archetype-migration row.

`js_for_of_next(iter)` replaces the pair. A builtin Map/Set iterator advances in
place through the same dispatcher arm the manual path uses, override probe
included, and reuses a `{value, done}` object cached in the iterator's sixth
field. Every other receiver — array iterators, generators, user iterators —
takes a generic arm that is the two-call shape it replaces. Manual `.next()`
calls and both public dispatchers keep allocating fresh results, so a caller
that retains one still observes spec behaviour.

The override probe no longer builds the prototype tower to decide whether `next`
was patched: the only route to the prototype object is `Object.getPrototypeOf`,
which materializes the tower, so a null tower proves no override. That removes a
key-string allocation from the manual path too.

Recycling is sound because the cached object is only reachable from the
compiler's `for…of` desugar, whose result is a temporary the loop body cannot
name and whose `done`/`value` are read before the next advance — the desugar
binds the loop variable ahead of the body, so even two loops sharing one
iterator cannot clobber an unread value.

One spec tightening: the sync desugar arm that previously skipped result
validation now routes through the fused entry, which validates on the generic
arm — matching the other sync driver.
