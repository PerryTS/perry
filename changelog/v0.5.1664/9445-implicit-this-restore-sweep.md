**Every runtime callback site now restores the caller's `this` through a GC
root** (#9445) — the sweep PR #9444 asked for after fixing four accessor sites
for #9417.

The runtime binds a callback's receiver by writing the GC-rooted
`IMPLICIT_THIS` cell and keeping the previous occupant in a **bare Rust local**
for the duration of the callback:

```rust
let prev = js_implicit_this_set(receiver);
… user code, which allocates …
js_implicit_this_set(prev);               // pre-collection address
```

That local is the caller's receiver, and the collector cannot see or rewrite
it. An evacuating young-gen minor inside the window — perry's default GC since
PR #7019 — relocates the caller's object, and the restore reinstalls a retired
from-space address as the caller's `this`. Nothing faults: the caller's next
`this.<field>` fails the object-type check on the recycled cell and answers
`undefined`, so the member access after it throws a TypeError naming a
property nowhere near the defect (#9417's `Cannot read properties of
undefined (reading 'def')`).

The issue counted ~20 sites; a grep of the whole runtime finds **122**
unrooted save/restores in 65 files (one of them landed with #9518 while this
sweep was in flight) (timers, node streams, dgram, cluster,
`fs.watch`, `EventTarget`, `Map`/`Set`/`URLSearchParams.forEach`, promisify,
JSON `toJSON`/replacer/reviver, ToPrimitive and ToPropertyKey, the iterator
protocol, Proxy traps and `Reflect`, bound functions, `super.x`, static
dispatch, …). Every one is now the idiom `prototype_chain.rs` and PR #9444
already use: root the saved value in a `RuntimeHandleScope` and re-read it at
the restore. Nine sites were already rooted; `dyn_eval/bridge.rs` roots
through its own stack. None of the 122 could be left alone — every one calls
user code (a closure, a class accessor or static method, a Proxy trap, a
`then`), which can allocate. Callback loops (`Map`/`Set`/`URLSearchParams.forEach`,
`EventTarget` dispatch, the emitters, watchers and timer batches) root the
displaced receiver **once per loop** rather than once per callback, and sites
that already own a `RuntimeHandleScope` reuse it, so the hot per-callback cost
is one handle read. Three sites also consumed their receiver again *after* the
call (`intl_subclass_super`, `temporal_subclass_super`, and the `process.stdin`
listener loops); those re-read it through a root too.

**Also fixed, same family, found by the fixture:** `JSON.stringify(value,
replacerFn)` handed the walk a **raw replacer closure pointer** after the
root-level replacer call (and the root `toJSON`) had run user code
(`json/replacer.rs`, both the pretty and the compact entry points). With an
allocating replacer this was a SIGSEGV in `js_closure_call2` — the walk called
a retired closure — and it survived the `prev` rooting alone. The closure and
the `""` key are now rooted across those calls.

**Test.** `test-files/test_gap_9445_implicit_this_restore_sweep.ts` — 34
cases, one per synchronously reachable site family, each a `function`-method
on a fresh young object that drives the site with an allocating callback and
then reads `this`. Deterministic, no GC env knobs; the PR description records
which cases print a non-zero `bad=` count on unfixed `main`. Event-loop-driven
sites (timers, `process.stdin`, dgram, cluster, `fs.watch`, pty, child
process) only see a heap `prev` from a nested pump and have no synchronous
reproduction; they carry the same mechanical fix. Two further candidate cases
(a `defineProperty` accessor on a typed array, a `toISOString` override on a
`Date`) diverge from node for a reason unrelated to rooting and are filed as
#9529; a `util.callbackify` case crashed the microtask pump at exit on every
build (a promise-side rooting bug, filed separately) and is not in the file.
