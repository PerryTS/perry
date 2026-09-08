# String constructor statics with spread arguments

This self-contained JS fixture exercises direct, computed and detached
`String.fromCodePoint` calls; empty and mixed spreads; array, typed-array and Set
operands; evaluation order; invalid code points; and the equivalent spread paths
for `String.fromCharCode` and `String.raw`.

Run `node --expose-gc main.js` for the reference, then compile `main.js` with Perry
and run the resulting executable. All 20 cases must pass, including 36 forced
collections during spread-operand evaluation. No Claude Code files, Bun
executable, credentials, network access, or generated assets are required.

The original HIR static-call fast paths discarded spread markers. An array was
passed as a scalar code point (throwing `RangeError: Invalid code point NaN`),
mixed char-code arguments became NUL characters, and raw substitutions became
comma-joined strings. Spread forms must use the reified variadic callable via
`CallSpread`, keeping the `String` constructor receiver and each spread boundary.
Do not make an ordinary array argument implicitly spread: the negative control
`String.fromCodePoint([65, 66])` must continue to throw.

The lowering contract is independently checked by
`crates/perry-hir/tests/string_static_spread.rs` (18 AST forms).
