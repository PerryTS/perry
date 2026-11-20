**test(repsel): drive the #7480 element-shape revocation matrix through real mutators**

#7496 shipped the per-array homogeneous element-shape invariant with a
bulk-mutator test that called `rebuild_array_layout` **directly**. Its comment
named seven mutators (`shift`/`unshift`/`splice`/`fill`/`copyWithin`/`reverse`/
`sort`); it exercised none of them. Any one could have stopped reaching the
funnel with the suite still green — CLAUDE.md's fourth way a gate cannot fail,
*"the gate runs but its subject never did"*.

That gap mattered more than usual because of what consumes the invariant next:
the #5093 versioned-loop clone hoists a guard into the preheader and emits
**unguarded** element reads in the cloned body. A mutation family that silently
stops revoking is not a slow path there — it is a miscompile that reads a `B`
through an `A`'s layout.

`crates/perry-runtime/src/array/element_shape_matrix_tests.rs` adds 25 tests,
each driving a **real FFI entry point** against a proven array. Each revoke
also asserts the **global epoch advanced** (the word a hoisted guard re-reads,
so a revoke it does not advertise is a revoke the consumer misses), and each
permutation/same-class family asserts it **re-proves with a fresh identity** —
separating a conservative revoke from genuine heterogeneity. Two vacuity
guards: the fixture must be proven *before* every op, and the in-place mutators
must return the array that was proven (otherwise a fresh, trivially unproven
array satisfies the revoke assertion while revoking nothing).

**The audit found no unhooked path — the shipped invariant is sound.** Two
findings are worth recording:

- **`sort`'s default path does not revoke through `rebuild_array_layout`.** It
  is a rank permutation written back via `RootedArrayElems::set`, so it revokes
  through the *store* funnel. Established by sabotage, not by reading: removing
  the revoke from `rebuild_array_layout` leaves the sort test green; removing it
  from `layout_note_slot` turns it red. `rebuild_array_layout`'s own doc comment
  claims otherwise.
- **Equal-length `splice` has exactly one guard.** `arr.splice(1, 1, other)`
  leaves `length` unchanged, so the structural `verified_len` check cannot see
  it, and the inserted item is a bare `ptr::write` that never reaches the store
  funnel. Only splice's own `rebuild_array_layout` catches it.

Sabotage-verified three ways: dropping the revoke from `rebuild_array_layout`
turns 7 tests red; dropping the store funnel from `layout_note_slot` turns 39
red; dropping it from `js_array_splice` alone turns **exactly one** red — the
equal-length splice test, confirming it is the unique guard for a case no other
mechanism covers.

Test-only: one new `#[cfg(test)]` file plus its `mod` declaration. No runtime
or emitted-code change, so no standing cost and no behaviour to A/B.
