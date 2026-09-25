### Performance

- **Functions that only read `arguments` no longer build an Arguments object
  (#10509).** #8807 already skipped the object when the body's only use was
  `arguments.length`. The proof in `perry-codegen` `codegen/arguments.rs` now
  also admits `arguments[k]` reads in value position — the Babel/TypeScript
  default-parameter output (`arguments.length > 0 && arguments[0] !== undefined
  ? arguments[0] : {}`, validator's `util/merge`), the Babel rest-parameter
  loop, `arguments[arguments.length - 1]`. The slot keeps the argument bundle
  the caller already passes, and:
  - `arguments.length` is the bundle length, read once in the prologue into an
    entry-block slot (it used to be a property-IC miss per read);
  - `arguments[k]` calls `js_arguments_bundle_index_get`, which answers an own
    element (an exact array index below the length) straight from the bundle
    and `TAG_HOLE` for every other key. That cold arm calls
    `js_arguments_bundle_get_slow`, which builds the object the prologue would
    have built — same callee, same restricted-callee thrower — and performs an
    ordinary `[[Get]]`, so `"callee"`, symbols, `Object.prototype` names and
    out-of-range or fractional indices read exactly as before (and an
    out-of-range index still never sees `Array.prototype`).

  The proof stays fail-closed: every reference HIR's collector counts must be
  a `.length` or `[k]` read the traversal also sees; a read in reference
  position (`delete arguments[k]`, `arguments[k]()` with the object as `this`)
  rejects; index reads also need the binding uncaptured by closures and, for a
  sloppy mapped object, every aliased parameter provably unassigned and not
  re-bound by a `var` (`rebound_locals`). Parameter defaults are scanned with
  the body. An elided object's mapped parameters are no longer boxed. The
  number-context and symbol-IC tiers defer to the new lowering for an elided
  receiver.

  `node args.js read_arguments 500000` from the issue: 3,464 ms → ~51 ms
  (`read_named` control: ~14 ms). The remaining gap is the caller's bundle
  allocation, which every rest-bundled call pays.

- The strict-mode `callee` thrower is configured once per thread instead of on
  every strict Arguments object: its frozen header bits are the last setup
  step, and every side table the earlier steps write is rekeyed when the
  closure moves. Takes `thrower_closure_value` from 7.4% to 0.25% of the
  issue's `escape_arguments` profile. Escaping Arguments objects otherwise keep
  their per-object descriptor-table entries; a shape-fixed representation for
  them is still open under #10509.

### Fixed

- `delete arguments.length` in a strict function that otherwise only reads
  `arguments.length` threw `TypeError: Cannot delete property`: the #8807
  elision kept the raw bundle, whose `length` is non-configurable. A delete, or
  a call through an `arguments` member, now keeps the real object.

Coverage: `test-files/test_gap_10509_arguments_reads.ts` (strict, every key
shape incl. `callee`, symbols, `-0`, `NaN`, `"01"`, inherited and
out-of-range indices, receiver/delete/capture/default/method/async shapes,
20k cold reads under nursery churn) and
`test_gap_10509_arguments_reads_sloppy.cts` (mapped parameters: untouched,
assigned, `++`, assigned from a closure, `var`-re-declared; computed `callee`
of a declaration and of a function expression), unit tests in
`codegen/arguments.rs` (`elision_tests`) and
`perry-runtime` `object/arguments_bundle_tests.rs`.
