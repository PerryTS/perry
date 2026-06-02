// #1816 / #321: a cross-module function that references `arguments` returned
// `undefined` for calls with fewer args than its inflated param count.
//
// Perry appends a synthetic `arguments` rest param (`name:"arguments",
// is_rest:true`) to any function whose body uses `arguments` (#677). The
// cross-module call path (`extern_func.rs`) treated it like a real `...rest`
// and bundled only the *trailing* args (after the fixed params) — but
// `arguments` must reflect ALL passed args. So `firstOrCount(5)` saw an empty
// `arguments` and returned undefined.
//
// This was the gateway blocker for the Effect framework (#321): effect's `pipe`
// and `dual` both use `arguments`, so they returned undefined cross-package,
// cascading through map / fiber runtime / ParseResult.
//
// Fix: `extern_func.rs` bundles ALL args (from index 0) into a synthetic
// `arguments` param (propagated cross-module via
// `imported_func_synthetic_arguments`), matching the same-module path and JS
// `arguments.length` semantics.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

import {
  variadicSum,
  firstOrCount,
} from "./_helpers/uses_arguments_mod.ts";

// (1) all-via-arguments (0 named params) — already worked, guard it.
console.log("(1) variadicSum(1,2,3):", variadicSum(1, 2, 3));
console.log("(1) variadicSum():", variadicSum());

// (2) named params + arguments, UNDER-filled call (the bug): fewer args than
//     the (inflated) param count.
console.log("(2) firstOrCount(5):", firstOrCount(5));

// (3) named params + arguments, fully-filled.
console.log("(3) firstOrCount(5,3):", firstOrCount(5, 3));
