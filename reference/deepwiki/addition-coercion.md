# Addition Coercion Reference Notes

Full DeepWiki responses:

- `reference/deepwiki/addition-coercion-engine262.md`
- `reference/deepwiki/addition-coercion-boa.md`

## engine262

- `Evaluate_AdditiveExpression` is dispatched from `src/evaluator.mts` for `AdditiveExpression` nodes. The model follows the spec shape: evaluate the left operand, apply `GetValue`, evaluate the right operand, apply `GetValue`, then perform addition semantics.
- Assignment expressions dispatch through `Evaluate_AssignmentExpression`; the important invariant for `(y = 1) + y` is that assignment returns the assigned value after storing it, so the left operand contributes `1` and the later `GetValue(y)` also reads `1`.
- `ToPrimitive(input, preferredType)` checks `@@toPrimitive` first, calls it with the correct hint, rejects object results, and otherwise falls back to `OrdinaryToPrimitive`.
- Addition uses the default/number ordinary hint, so ordinary objects try `valueOf` then `toString`. String coercion uses the string hint, so ordinary objects try `toString` then `valueOf`.
- Ordinary object default stringification is `"[object Object]"`; function stringification routes through the function `toString` path rather than a malformed object tag.

## Boa

- Boa's `JsValue::add` has fast paths for numeric, BigInt, and string/string addition, then a slow path that converts both operands with `to_primitive(context, PreferredType::Default)`.
- After `ToPrimitive`, if either primitive is a string, Boa concatenates `ToString(left)` and `ToString(right)`; otherwise it converts both primitives to numeric and performs number or BigInt addition.
- `compile_assign` keeps simple assignment expression results in the destination register used by the surrounding expression. That preserves `(y = 1)` as the value `1` before the right-side `y` is evaluated.
- `JsObject::to_primitive` checks `[Symbol.toPrimitive]`, rejects object results, and falls back to `ordinary_to_primitive`. Its ordinary method order is `valueOf`, `toString` for default/number and `toString`, `valueOf` for string.
- `Function.prototype.toString` has a dedicated implementation for callable objects; ordinary object default stringification remains the standard `"[object Object]"`.

## Perry Regression Targets

- `language/expressions/addition/S11.6.1_A2.4_T4.js`: ensure the assignment expression in `(y = 1) + y` stores before the following read and evaluates to the assigned value, producing `2`.
- `language/expressions/addition/S11.6.1_A3.2_T1.2.js`: ensure object/function default addition follows `ToPrimitive(default)` and string-concatenates the resulting `ToString` values without producing malformed `"[object Object]}"` text.
