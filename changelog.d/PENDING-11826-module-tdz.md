Module-level `let` and `const` bindings now preserve their temporal dead zone.
Reads before initialization throw `ReferenceError` before evaluating later
parts of the expression, matching Node for computed compound assignments and
for calls through hoisted functions.
