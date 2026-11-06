// #5908 (test262 built-ins/Function worklist): the `Function` constructor's
// [[Prototype]] is `%Function.prototype%`, so a property installed on
// `Function.prototype` is readable off the constructor object itself
// (`Function.prototype.indicator = 1` ⇒ `Function.indicator === 1`, test262
// S15.3.3_A2_T2).
//
// The HIR member-lowering was collapsing `Function.<unknown>` to
// `globalThis.<unknown>`, dropping the constructor receiver, so the inherited
// read resolved against `globalThis` and came back `undefined`. This is the
// same receiver-drop already fixed for `RegExp` (#5897, S15.10.5_A2_T2);
// `Function` is the same safe case — nothing downstream keys on the collapsed
// `GlobalGet(0).<prop>` shape for it, and its `.name` / `.length` /
// `.prototype` reads are handled by dedicated arms. Keeping the receiver lets
// the runtime walk the prototype chain
// (`closure_get_dynamic_prop`'s `function_prototype_fallback_target`), which
// already resolves the inherited property.
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// --- the fix: inherited read off the Function constructor -----------------
(Function.prototype as any).indicator = 1;
console.log("Function.indicator :", (Function as any).indicator); // 1

// --- inherited read off ordinary function instances (already worked;
//     guards that keeping the receiver didn't disturb the closure path) ----
function userFn() {}
const arrow = () => {};
console.log("userFn.indicator   :", (userFn as any).indicator);            // 1
console.log("arrow.indicator    :", (arrow as any).indicator);             // 1
console.log("bound.indicator    :", (userFn.bind(null) as any).indicator); // 1

// --- regression guards: dedicated static-member arms still resolve --------
console.log("Function.name      :", Function.name);   // "Function"
console.log("Function.length    :", Function.length); // 1

// --- reassignment on the shared prototype is observed through the ctor ----
(Function.prototype as any).indicator = 42;
console.log("after reassign     :", (Function as any).indicator); // 42
