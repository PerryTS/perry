### Performance

- **`new Promise(executor)` no longer installs per-closure metadata on its
  resolving functions.** Each `resolve`/`reject` pair used to get an eagerly
  built `name` string, a closure dynamic-prop entry, a descriptor-table entry, a
  `length` entry and a non-constructable entry, and the collector pruned all of
  it again when the pair died. The descriptor install also bumped the global
  property-plan epoch twice per promise. That invalidated the cached "no `then`
  on `Object.prototype`" verdict and every inherited-read cache, so the next
  resolution recomputed them. The facts involved (`length` 1, `name` "", no
  `[[Construct]]`) belong to the function kind, so they now come from it. The
  arity already registered for the resolving thunks answers `length`, the
  absent func-ptr name answers `name` as `""`, and a new `NON_CONSTRUCTOR` bit on
  the closure body record answers `new resolve()` with a TypeError. The
  entry-less descriptor defaults (`writable: false, enumerable: false,
  configurable: true`) were already what the spec requires. The same change
  covers `Promise` subclass construction, `NewPromiseCapability`, the
  thenable-job resolving functions, the `Promise.all`/`allSettled`/`any` element
  functions (two side-table inserts per element) and the
  GetCapabilitiesExecutor. Refs #10521.

  Measured on the issue's awaited-promise benchmark (Linux x64, perry-dev
  build, `PERRY_NO_AUTO_OPTIMIZE=1`): the executor form drops from ~46.7k to
  ~13.2k instructions per promise (callgrind), against ~7.8k for the
  `Promise.resolve`/`Promise.reject` control, which is unchanged. Wall time for
  300k promises goes from 3.3–4.7 s to 0.62–0.66 s for both executor variants.
  The remaining gap to the control is mostly the generic closure capture-store
  bookkeeping (`layout_note_slot` plus the write barrier, ~2.4k instructions
  per promise for the four capture stores), which every closure pays.

### Fixes

- **The thenable job's resolving functions and the GetCapabilitiesExecutor are
  no longer constructors.** `new resolve()` on the functions a thenable's `then`
  receives now throws a TypeError, and neither they nor the capability executor
  report an own `prototype` any more, matching Node.
- **`Object.defineProperty(fn, "name" | "length", { value })` keeps the
  intrinsic attributes.** Redefining a function's built-in `name` or `length`
  with a descriptor that omits `writable`/`enumerable` used to merge into an
  assumed `{writable: true, enumerable: true}` data property, which made the
  key writable and enumerable (`Object.keys(fn)` listed it). It now merges
  into the intrinsic `{writable: false, enumerable: false, configurable: true}`,
  as for ordinary and bound functions in Node.
