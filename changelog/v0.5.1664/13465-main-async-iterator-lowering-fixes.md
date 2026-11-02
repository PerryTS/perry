Fix node divergences and stale tests on main.

- Built-in iterator receivers: the shape-store key-add funnels now honor the
  #9019 reserved-field floor, so `it.foo = 1` on a Map/Set/array/string
  iterator no longer overwrites its backing-collection field.
- A Map or Set constructed by a derived class records its prototype; spread,
  `Array.from` and for-of copy its entries only when that prototype is the
  intrinsic one, so an overridden `[Symbol.iterator]` drives the iteration.
- `Array.from(array)` walks the array directly only while the array's
  iteration is intrinsic, including `%ArrayIteratorPrototype%.next`.
- Iterator GetMethod reads a key's storage slot, not its position, so an
  accessor redefined on `%ArrayIteratorPrototype%.next` is called.
- `await` and promise resolution read a promise's own `constructor` (and the
  intrinsic `then`'s SpeciesConstructor read) as PromiseResolve requires.
- A promise reaction enters its promise's async scope only while a promise
  hook observes promises; otherwise it runs in the enclosing execution
  context, as in Node (`executionAsyncId()` before any hook is enabled).
- The entry's tick/microtask checkpoint follows Node's module format: the
  extension or the nearest package.json `"type"`, then module syntax.
- `zlib.brotliCompressSync(buffer, options?)` in the API manifest.
- Stale perry-hir and perry-codegen test fixtures; runtime warnings;
  `PERRY_PIP_SABOTAGE` registered as a build-cache input.
