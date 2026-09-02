### Fixed

- **`util.format("%s", …)` now inspects an object that has no user
  `toString`,** matching node. Perry applied `String(value)` unconditionally, so
  `%s` on `[1, , 3]` gave `1,,3` where node gives `[ 1, <1 empty item>, 3 ]`, and
  `%s` on any object gave `[object Object]`. Node's rule (`hasBuiltInToString` in
  `lib/internal/util/inspect.js`) is: a number or bigint formats numerically, a
  non-object goes through `String()`, and an object goes through `String()` only
  when its `toString` is USER-defined — otherwise it is inspected with
  `{ depth: 0 }`, which is why a nested object collapses to `[Object]` rather
  than being walked.

  The discriminator is the subtle part, and "resolves to a callable `toString`"
  is **not** it: perry installs `Object.prototype.toString` as a real,
  discoverable closure (`install_proto_method`), so every ordinary object
  resolves one and a callable test answers "user-defined" for `{ a: 1 }`. The
  implemented test compares the resolved closure's `func_ptr` against the
  built-in thunk — exactly node's built-in question, and it stays right for
  `obj.toString = Object.prototype.toString`, which node also inspects. Arrays,
  Maps and Sets carry no user `toString` in this model and always inspect.

- **`util.format("%o", …)` now carries the `showHidden` surface for arrays.**
  `%o` is `util.inspect(value, { showHidden: true, depth: 4 })`, and an array's
  own `length` is a non-enumerable property, so node prints `[length]: N` after
  the elements of every array at every depth — including nested ones
  (`{ a: [ 1, 2, [length]: 2 ] }`) and the empty array, where `%o` is
  `[ [length]: 0 ]` while `%O` is a bare `[]`. Perry omitted it entirely. The
  `showHidden` plumbing already existed and already produced node's `[hidden]: 9`
  bracket form for non-enumerable object properties; only the array tail was
  missing.

  Node's `groupArrayElements` column layout stops applying once a non-index
  entry is appended — measured: `%o` on `[1 … 12]` stays on ONE line where the
  same array under `%O` breaks into right-aligned columns — so the tail's
  layout uses the single-line form whenever it fits the break length.

  Affected files:

  - `crates/perry-runtime/src/builtins/formatting/util_format.rs` — the `%s`
    policy and its built-in-`toString` test.
  - `crates/perry-runtime/src/builtins/formatting/value_repr.rs` — the
    `[length]` entry and its layout.
  - `crates/perry-runtime/src/builtins/formatting.rs` — both array walks (the
    top-level one and the `format_jsvalue_for_json` twin that renders an array
    reached as an object FIELD) emit the tail under `showHidden`.

  `%O` is untouched and pinned as a control throughout, as is `%s` on every
  primitive and on the two objects that DO define their own `toString`.

  Validation: `test-files/test_gap_9463_util_format_s_o_policy.ts`, byte-compared
  against node 26.5.1 — 20 of its 58 lines diverge on unfixed `origin/main`, none
  after. `util_format.rs` also unit-tests the `%s` predicate directly over
  primitives, an array, a Map, a Set, a plain object (must inspect) and an object
  with an own non-builtin `toString` (must not) — the pair that fails in one
  direction or the other if the built-in-thunk comparison is replaced by a bare
  callable test.

  Measured but deliberately NOT changed, each a separate pre-existing divergence
  called out in the fixture header: `%s` on a BigInt (node's `formatBigInt` keeps
  the `n` suffix, perry prints `5`); `%s` on a Date (node inspects to the ISO
  form) or an Error (node prints the stack), neither of which is an ordinary
  object in perry's model; `%s` on a class (#9468); and `%o` on an object nested
  four deep, where node's `compact: 3` rule breaks the line and perry keeps it on
  one.
