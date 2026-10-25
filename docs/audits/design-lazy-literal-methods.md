# Lazy object-literal methods (design note, not built)

Status: proposal from the #12016 method-value lane. Nothing here is
implemented.

## The cost

`qs` calls `side-channel`'s `getSideChannel()` on every `stringify` and on
each nested object. Each call evaluates an object literal of five function
expressions (`assert`, `delete`, `get`, `has`, `set`) that close over the
call's `channel` and `$channelData`. `side-channel-weakmap` and
`side-channel-map` do the same one level down. On the qs workloads this is
about 6.7 M closure allocations per run. Almost all of them are only ever
*called* through the literal (`channel.get(key)`); their function identity is
never observed.

The closures are the program's own observable values: `channel.get` must be a
function object, `channel.get === channel.get` must hold, and
`Object.values(channel)`, spread, `JSON.stringify`, `Object.getOwnPropertyDescriptor`,
`for…in`, a debugger and `structuredClone`'s error path all see them. So the
compiler cannot drop the allocation. It can delay it until something observes
the value.

## What the shape already knows

The ConstFn birth shapes (`codegen/static_constfn.rs`, `ConstFnBirth`) record,
for each function-valued slot of a literal, the body symbol that slot holds at
birth. The shape is therefore already the authority for "slot k of this
object runs body B". What the slot itself still has to supply is the
per-call part: the captured environment.

## Proposal

1. **Slot content.** At birth, a lazy slot holds the literal's shared
   capture environment (one boxed env object per literal evaluation, or the
   enclosing frame's existing capture box), tagged as *unmaterialized*. One env
   serves every lazy slot of the literal, so a five-method literal allocates
   one object (the env) instead of five closures.
2. **Shape lane.** The birth shape marks those slots with a new lane kind,
   `LAZY_FN(body)`, next to the existing ConstFn lane. The lane is a shape
   fact: a store to the slot (an ordinary assignment) is a shape transition
   to a plain data lane, as any representation change already is.
3. **Calls.** A call site `o.k(args)` whose shape compare hits a `LAZY_FN`
   lane calls body B directly with the env loaded from the slot and `this` =
   `o`. That's one shape compare and one load, with no closure.
4. **Reads.** Every read of the slot as a value goes through one function,
   `materialize_lazy_slot(o, k)`. It allocates the closure (body B, env),
   stores it into the slot, and moves the object to the plain-data shape (the
   same transition a store makes). Identity holds from then on because the
   slot holds the one materialized function. The function belongs to the
   object model's slot-read primitive. It is not a per-call-site check. Every
   reflective path (`Object.values`/`entries`, spread, `JSON`, descriptor
   reads, `for…in` value reads, inspection, structured clone) already funnels
   through slot reads keyed by shape lane, and the lane says "materialize".
5. **GC.** The unmaterialized tag is a pointer to the env object, so it's
   traced like any pointer slot. No new root kind is needed.

## Why it isn't in the #12016 lane

- Every raw slot reader in the runtime must honour the new lane before the
  first lazy literal is created. A single reader that copies the tagged env
  out as if it were a value is a correctness bug, and the bug is silent. The
  work is a whole-object-model invariant: a census of slot readers plus a
  sabotage test that turns a reader red when it skips the lane. It is not a
  method-value read fix.
- The body ABI must accept an env from the slot rather than from a closure
  header. ConstFn bodies are closure bodies today, so a direct call needs a
  thin entry that builds no closure.
- `this`-capturing methods and arrows must stay eager (the existing ConstFn
  admission already refuses rebindable clones).

## Acceptance

- qs parse/stringify: closure allocations drop by the number of
  never-observed literal methods (the 6.7 M), output identical to node.
- Gap tests: identity after first read, `Object.values`/spread/JSON/descriptor
  reads of a never-called literal, assignment over a lazy slot, `delete`,
  `Object.freeze` before first read, and a GC between birth and first read.
- Sabotage: a slot reader that skips materialization must turn a gap test red.
