An own method now beats a Date or Array builtin on a receiver whose kind the
compiler did not prove, closing #11493. `const d: any = new Date(0);
d.getTime = () => 42; d.getTime()` printed `0`, where node prints `42`.

#10943 added the own-override test for receivers whose KIND is proven. The
other half was #10476's receiver-kind guard, which serves UNPROVEN receivers
(an `any` local, an untyped parameter, a property read, a call result): it
checks at runtime that the value is a Date or a plain array and then called
the builtin directly. A matching kind says nothing about own properties, so
every Date method that guard lowers ignored an own replacement, as did its
`toLocaleString()` arm and its array methods (`toSorted`, `toReversed`,
`toSpliced`, `reduceRight`, `copyWithin`). A non-callable own `getTime`
did not throw either.

`lower_call/property_get/builtin_kind_guard.rs` now sends a heap receiver that
passes the Date or plain-array check through `emit_own_override_branch`
before the builtin; one that may own the name takes the universal dispatcher.
Numbers and Symbols are primitives and skip it. The predicate may allocate,
so when it is emitted the receiver and every argument are rooted across it and
re-read in each arm.

Closed on the proven path too:

- `own_override_guard.rs` takes its Date names from the kind guard's own
  `date_builtin` table instead of a hand-kept list that stopped short, so a
  proven Date's `getUTCHours`, `setTime`, `toUTCString`, ... get the diamond;
  it also covers the five array names above.
- `expr/folded_builtin_override.rs` gains the Date folds that were missing
  from the #10943 table: `getTimezoneOffset`, `toJSON`, `toDateString`,
  `toTimeString` and the three `toLocale*String`s. A numeric receiver of the
  shared `DateToLocaleString` node keeps the plain fold.

Not covered: `Expr::DateToUTCString`. HIR folds `toUTCString` and
`toGMTString` into that one node, so it does not know which name to test.

Cost, callgrind, two unproven Date calls per iteration: 259 -> 280
instructions with the global install flag clear; 259 -> 1891 with it armed,
which matches the 1853 a proven Date already pays under #10943.

Validation: `test_gap_11493_own_method_beats_date_builtin.ts` differs from
node on 71 of 89 lines on main and on none with this change (three TZs);
`cargo test -p perry-codegen` green; 0 regressions over 133 related existing
tests; `gc_root_dominance_check.py --moving-only` 0 violations.
