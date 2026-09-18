Fixed `Math[k]`, `JSON[k]`, `Object[k]`, `Number[k]`, `Reflect[k]`, `Date[k]`, `String[k]`, and
other built-in namespace/constructor member reads with a *variable* (non-literal) computed key
reading a property of the number `0` instead of the real object — `Math[key](x)` threw
`(number).x is not a function`, and `typeof Math[key]` was `undefined` for every key. #973's
value-form reroute of bare built-in identifiers was correctly undone in member-object position for
a statically-known member name (`Math.max(...)`), so the intrinsic call/constant-fold paths could
keep their pre-#973 `GlobalGet(0)` receiver — but the undo also fired for a computed non-literal
key, where nothing can resolve to an intrinsic at lowering time, collapsing the receiver to the
bare `GlobalGet(0)` sentinel. Closed #6677 fixed only the string-literal direct-call form
(`Math["max"](...)`); this fixes the variable-key forms (call and value-read position, keys from a
variable, a template literal, or a function return) it left broken. Static (literal) key paths are
unaffected by construction. `#10483`
