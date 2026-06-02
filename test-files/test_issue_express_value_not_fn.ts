// Regression test for the express `TypeError: value is not a function` crash.
//
// Express 5.x's CJS-wrapped `node_modules/express/lib/utils.js` IIFE body has
// shape (paraphrased):
//
//   (function() {
//       function require(specifier) { ... return _req_N; }
//       var { METHODS } = require('node:http');
//       ...
//   })();
//
// Perry's `lower_fn_expr` previously bucketed the body into
// `var_hoisted + func_decls + exec_stmts`, which reordered the var
// declaration (with its initializer) to run BEFORE the function declaration
// in the lowered HIR. The `require('node:http')` call then ran when `require`
// was still null, and the closure dispatcher's null-callable check threw
// `TypeError: value is not a function` — express's module init died on the
// very first import.
//
// Per JS spec, function declarations are FULLY hoisted (binding + value)
// at function entry, while `var` declarations only hoist their BINDING
// (to `undefined`); their initializer expressions run at source position.
// `lower_fn_expr` now emits `func_decls` ahead of all other statements and
// keeps `var` initializers in source order, matching that spec.

(function () {
    // Function declaration FIRST in source order, var-destructure SECOND.
    // Both forms (with and without destructuring) must work.
    function getThing(): { value: number } {
        return { value: 42 };
    }

    var { value } = getThing();
    console.log("value:", value);

    var plain = getThing();
    console.log("plain.value:", plain.value);
})();

// Run as an IIFE assigned to a const (matches express's `const _cjs =
// (function(){...})();` shape exactly).
const result = (function () {
    function reqHelper(spec: string): { items: number[] } {
        if (spec === "list") return { items: [1, 2, 3] };
        return { items: [] };
    }

    var { items } = reqHelper("list");
    return items.length;
})();

console.log("result:", result);
console.log("OK");
