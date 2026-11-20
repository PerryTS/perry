**A spread method call on an imported receiver now dispatches the receiver's method** (#7191).

```ts
import { arr, nums } from "./lib.ts";
arr.map(...[cb])        // was: TypeError
arr.slice(...[1, 3])    // was: [3,1,2]  — the whole array
nums.join(...["-"])     // was: TypeError
nums.includes(...[20])  // was: false
```

Four cases, four wrong, and two of them wrong *silently* — which is why it lasted. The spread argument list was arriving as a single array in the builtin's first slot, so `slice` saw one non-numeric argument and `includes` compared against an array.

`Expr::ExternFuncRef` is not one shape. It covers an imported **function**, where a method-apply dispatch would be wrong — which is why the generic `CallSpread` tail skips it — and an imported **value**, where method-apply is exactly what is needed. The skip did not distinguish the two, so a spread call on any imported receiver declined to dispatch. `ctx.imported_vars` already records precisely this distinction ("names of imports that are exported variables, not functions"), so the arm consults it instead of declining the whole family.

The imported-array fold that used to cover part of this was papering over the same gap, and papering badly — `slice` and `includes` were already returning wrong values through it.

`test_gap_spread_call_imported_receiver_7191.ts` asserts both halves, because the risk here is fixing one by breaking the other: the imported-value receivers that were broken (arrays via `map`/`slice`/`join`/`includes`/`indexOf`/`concat`/`at`, including a spread argument that is itself an array so "the args collapsed into one array" cannot pass by accident), and the imported-function/object/class receivers the skip exists to protect (`fn.call(...)`, `fn.apply(...)`, a direct `fn(...)`, an imported object's method, an imported class's static and instance methods). A local receiver is asserted unchanged.

The existing `test_gap_6718_spread_call_native_method` covers inline literals, local receivers and `queueMicrotask` — not imported receivers — which is why this shape had never been under test. It stays green, as do `test_gap_import` and the class/static families.
