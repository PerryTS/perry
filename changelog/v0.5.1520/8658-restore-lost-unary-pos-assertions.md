Restored three `computed_store_rooting_tests` assertions that were lost when
#8650 was squash-merged as part of #8657.

#8650 changed unary `+` on a non-numeric operand to emit `js_dynamic_pos`
instead of `js_number_coerce` (`expr/unary.rs`, `UnaryOp::Pos`) and updated the
three tests that assert on that helper. The squash kept the emission change and
dropped the test edit, so the assertions kept naming the old helper and failed
deterministically.

No product behaviour changes: the tests now name the helper the compiler
actually emits, which is what #8650 intended.
