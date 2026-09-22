### Fixed

- **`typeof o.toString` on a non-escaping object literal answered `undefined` (#10689).**
  Reading an *inherited* member as a VALUE — `o.constructor`, `o.toString`,
  `o.hasOwnProperty`, a user-added `Object.prototype` property, or a class's own
  prototype METHOD read as a value — answered `undefined` whenever the receiver
  was a scalar-replacement candidate, while *calling* the same member
  (`o.toString()`, `"" + o`, `` `${o}` ``) was correct. No error: the program
  took the other branch and continued.

  The mechanism is escape analysis, not the lazy `globalThis` realm the report
  guessed at. `collectors/escape_check.rs`'s `PropertyGet` arm classified every
  read on a candidate local as a "plain field read — safe", without checking
  that the class chain actually declares the key. The local stayed
  scalar-replaced (no heap object exists at all) and `expr/property_get.rs`'s
  scalar arm folded the slot-less read to the constant `undefined`. The three
  WRITE arms of the same analysis already carried exactly this rule — #9024 for
  `PropertySet`/`PutValueSet`, #9460 for `PropertyUpdate` — and the sibling
  object-literal analysis in `collectors/escape_objects.rs` has always had it;
  only the read arm was missing it.

  That also explains the reported order-dependence. `JSON.stringify(o)` earlier
  in the function "repaired" the read because passing `o` to a call makes it
  escape — not because it forced the realm. `JSON.stringify` of an *unrelated*
  object forces the realm just the same and did **not** repair it; that case is
  pinned as a test.

  `Expr::Call`'s arm no longer routes a fused method-call callee
  (`o.m()`) back through the `PropertyGet` arm, so the receivers that
  `simple_scalar_method_summary` deliberately keeps scalar-replaced still are.
  Measured instruction-neutral on the r0–r9 ladder (max |Δ| 0.02%, noise).

  Regression test: `crates/perry/tests/object_prototype_value_read_10689.rs`.
