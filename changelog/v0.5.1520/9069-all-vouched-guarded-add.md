A guarded `+` tree whose every leaf is provenance-vouched now lowers unguarded
instead of failing compilation.

Both the pi and cc application bundles died on `guarded + tree has no testable
leaf`. The shape is a compound add in a `for…of` loop whose RHS is a call: the
call leaf is flagged by `numeric_proof_is_declared_only` — a declared numeric
return routes the tree into `lower_guarded_numeric_add` — while its
integer-literal returns make the same leaf provenance-vouched by
`expr_produces_canonical_raw_f64`, so no leaf needed a runtime test and the
guarded lowering had nothing to guard.

Every leaf being vouched is exactly the case that needs no guard, so the tree
lowers directly.
