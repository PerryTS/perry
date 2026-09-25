Add the standard `get [Symbol.species]` accessor to the built-in
constructors: `Array`, `Map`, `Set`, `Promise`, `RegExp`, `ArrayBuffer`,
`SharedArrayBuffer` and `%TypedArray%` (which `Uint8Array`, `Int32Array` and
the other typed arrays inherit). Each is an own accessor
`{ get, set: undefined, enumerable: false, configurable: true }` whose getter
is named `get [Symbol.species]` and returns `this`. Before this, Perry
returned `undefined` for `C[Symbol.species]` on all of them. This is the
generic half of #11193; the `Buffer[Symbol.species]` (`FastBuffer`) half
landed in #11198.

The accessor is installed by `install_builtin_species_accessor`
(`perry-runtime/src/object/global_this/install_static.rs`), from
`populate_global_this_builtins` and `ensure_typed_array_intrinsic`.

The species-aware consumers now see the intrinsic constructor where they used
to see `undefined`. Two of them needed a fast-path check so their default
results don't go through a generic `Construct`. `TypedArraySpeciesCreate`
(`typedarray/species.rs`) maps the intrinsic same-kind constructor to its
same-kind allocation, and RegExp `SpeciesConstructor` (`regex/match_all.rs`,
used by `split` and `matchAll`) maps `%RegExp%` to the direct construct.
`ArraySpeciesCreate` and Promise `then`/`finally` already treated the
intrinsic as the default.

Still open: static symbol properties are not inherited by a
`class X extends Array` (or `Map`, `Promise`, …), so `X[Symbol.species]` is
still `undefined` for user subclasses, as it was before this change.

test262 species subset (395 cases, `--all-features`): 268 → 298 passes, no
regressions. Covered by `test-files/test_gap_11193_builtin_species_accessor.ts`.
