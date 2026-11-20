Fix a recursive `function*` declared inside another function throwing `TypeError: value is not a function` (#13029).

- The function-body hoisting pre-pass pre-defined a local for every nested function declaration, generators included. A generator that is not forward-referenced is lowered to a top-level `Function` plus a `FuncRef` binding, so its recursive self-call read the enclosing function's local slot instead. That pre-pass now skips such generators, matching the existing gate for function expressions; forward-referenced generators keep the closure path and their hoisted local.

Validation: `test-parity/node-suite/globals/nested-recursive-generator.ts` fails on the base commit and matches Node after the fix.
