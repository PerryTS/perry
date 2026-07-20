// Test: Symbol.for(key) ToString-coerces a non-string key (Perry gap #6681)
// Per sec-symbol.for step 1, `key = ToString(key)`, so Symbol.for(undefined)
// is Symbol.for("undefined"), Symbol.for(42) is Symbol.for("42"), etc.
// Run: node --experimental-strip-types test-files/test_gap_symbol_for_coercion.ts

// --- undefined coerces to "undefined" ---
const su = Symbol.for(undefined as any);
console.log(su.toString());                                 // Symbol(undefined)
console.log(String(su));                                    // Symbol(undefined)
console.log(su === Symbol.for("undefined"));                // true
console.log(Symbol.keyFor(su));                             // undefined  (the string key)

// --- number coerces to its decimal string ---
const s42 = Symbol.for(42 as any);
console.log(s42.toString());                                // Symbol(42)
console.log(s42 === Symbol.for("42"));                      // true
console.log(Symbol.for(0 as any) === Symbol.for("0"));      // true

// --- null coerces to "null" ---
const sn = Symbol.for(null as any);
console.log(sn.toString());                                 // Symbol(null)
console.log(sn === Symbol.for("null"));                     // true

// --- booleans coerce to "true"/"false" ---
console.log(Symbol.for(true as any) === Symbol.for("true"));    // true
console.log(Symbol.for(false as any) === Symbol.for("false"));  // true

// --- object coerces via ToString ---
console.log(Symbol.for({} as any) === Symbol.for("[object Object]")); // true
console.log(Symbol.for([1, 2] as any) === Symbol.for("1,2"));         // true

// --- distinct coerced keys stay distinct ---
console.log(Symbol.for(1 as any) === Symbol.for(2 as any)); // false

// --- Symbol key throws (ToString(symbol) is a TypeError) ---
let threw = false;
try {
  Symbol.for(Symbol("x") as any);
} catch (e) {
  threw = e instanceof TypeError;
}
console.log(threw); // true

console.log("ALL SYMBOL.FOR COERCION TESTS PASSED");
