**`new.target?.<prop>` short-circuits again in a function called without `new`**
(`test_issue_449_new_target` passes; its known-failure entry is deleted).

`function f() { new.target?.name }` called as `f()` threw
`TypeError: Cannot read properties of undefined (reading 'name')` instead of
evaluating to `undefined`. HIR lowering special-cased the spellings
`new.target.<prop>` and `new.target?.<prop>`. Both folds were written in May,
when `new.target` could not be read directly. #5061 (June) made bare
`new.target` lower to the runtime `Expr::NewTarget` and rewrote both folds into a
plain `PropertyGet { NewTarget, prop }`. For the `?.` form that dropped the
nullish guard, so outside a constructor it read a property off `undefined`.
The same fold also skipped the class-field-initializer rule, so
`x = new.target?.name` in a field initializer read the constructor's
new.target instead of `undefined`. The breakage stayed hidden because the
parity suite was dark at the time. It was then baselined as untriaged debt
(#8271).

Both folds are removed. Every `new.target` member form now goes through the
generic member and optional-chain lowering, over the same `Expr::NewTarget`
operand. `test_gap_449_new_target_member_forms` pins the result against node:
the `.`, `?.` and computed forms, bare `new.target`, and forms inside arrows,
in plain functions called with and without `new`, in class constructors
(including a subclass leaf), in methods and in field initializers.
