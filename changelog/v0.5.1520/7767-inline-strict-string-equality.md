### Strict `===` against a string literal is no longer a runtime call

`n.kind === "num"` — the shape every tree-walking interpreter, reducer and
discriminated-union dispatch is built out of — compiled to a `js_eq` →
`js_jsvalue_equals` call pair. On `gc-handoff/apps/interp.ts` that pair plus the
`memcmp` under it was **~21% of the program's runtime**, against 30% for the
user code itself.

The call was never necessary. When one operand is a string literal, *both* of a
string's runtime representations are known at compile time, so the whole
dispatch folds into a few integer ops:

* the pooled heap `StringHeader` (one per literal per module — `crate::strings`
  hoists literals to module init), so **pointer identity** settles the true case
  in a single `icmp`. `{ kind: "num" }` stores that very pointer, and GC
  evacuation rewrites the pool root and the object slot together, so identity
  survives collection;
* the SSO immediate — a compile-time constant for literals of ≤ 5 bytes.
  `charAt` and `JSON.parse` produce inline `SHORT_STRING_TAG` values, and
  `"+" === "+"` across those two representations still has to be true;
* every other NaN-box tag is a different ECMAScript value, so a number, an
  int32, a pointer (including a boxed `new String("x")`), a bigint,
  null/undefined/bool, or an SSO value with different bytes is decided *false*
  without touching memory;
* a heap string that is not the pooled pointer is compared by `byte_len` and by
  its first and last byte — all three compile-time constants, all inside bytes
  the length check has already proved the header owns. For literals of ≤ 2 bytes
  that settles it outright; only a same-length, same-endpoints heap string
  reaches `js_string_equals`.

The two string-equality arms with *no* literal operand (`names[i] === name`)
gained the two shortcuts that need no compile-time facts: identical bits, and
SSO × SSO with differing bits (the SSO encoding is canonical, so equal content
*is* equal bits). The pre-existing fallbacks are unchanged, which matters for
the legacy arm — it keeps `js_get_string_pointer_unified`'s number-coercing
behaviour for operands whose `string` annotation lies. That composition
*materializes* an SSO operand onto the heap, so routing SSO × SSO around it
removes two throwaway allocations per comparison as well as the calls.

Semantics are exact `===`, not the approximation the old `both_strings` arm
reached through `js_get_string_pointer_unified`: `NaN !== NaN`, `+0 === -0`,
distinct heap strings with equal contents are equal, int32 and double
representations of the same Number are equal, and `new String("num") !== "num"`.
`test-files/test_strict_eq_string_literal_inline.ts` pins those against Node,
including the multi-byte-UTF-8 cases where "first byte" is not "first
character". `expr/compare_tests.rs` is the IR census — the `streqlit.*` blocks
present *and* `js_eq` absent — with negatives for loose `==` (which coerces, and
must keep its helper) and for a comparison with no literal operand.
