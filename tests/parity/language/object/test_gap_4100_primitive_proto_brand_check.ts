// #4100 (part of #3662) — primitive-wrapper prototype methods must perform the
// spec `this` brand check and throw a TypeError on an incompatible receiver,
// and must return the correct value when invoked reflectively on a real
// primitive. Pre-fix these resolved to `Object.prototype` and returned
// "[object Object]"/"[object Symbol]" instead of throwing / the right value.
// We print only error *types* / values so output is byte-identical to Node.

function threw(fn: () => void): string {
    try {
        fn();
        return "NO_THROW";
    } catch (e: any) {
        return e && e.name ? e.name : String(e);
    }
}

// --- Brand check: wrong / primitive receiver must throw TypeError. ---
console.log("Number.valueOf{}:", threw(() => (Number.prototype.valueOf as any).call({})));
console.log("Number.toLocaleString{}:", threw(() => (Number.prototype.toLocaleString as any).call({})));
console.log("Boolean.toString{}:", threw(() => (Boolean.prototype.toString as any).call({})));
console.log("Boolean.valueOf{}:", threw(() => (Boolean.prototype.valueOf as any).call({})));
console.log("Symbol.toString{}:", threw(() => (Symbol.prototype.toString as any).call({})));
console.log("Symbol.valueOf{}:", threw(() => (Symbol.prototype.valueOf as any).call({})));
console.log("BigInt.toString{}:", threw(() => (BigInt.prototype.toString as any).call({})));
console.log("BigInt.valueOf{}:", threw(() => (BigInt.prototype.valueOf as any).call({})));

// Cross-brand: a Number is not a BigInt etc.
console.log("Number.valueOf on sym:", threw(() => (Number.prototype.valueOf as any).call(Symbol("x"))));
console.log("BigInt.valueOf on 5:", threw(() => (BigInt.prototype.valueOf as any).call(5)));

// --- Correct receiver: reflective dispatch returns the right value. ---
console.log("Number.valueOf(5):", (Number.prototype.valueOf as any).call(5));
console.log("Number.toLocaleString(5):", (Number.prototype.toLocaleString as any).call(5));
console.log("Boolean.toString(true):", (Boolean.prototype.toString as any).call(true));
console.log("Boolean.valueOf(false):", (Boolean.prototype.valueOf as any).call(false));
console.log("Symbol.toString:", (Symbol.prototype.toString as any).call(Symbol("x")));
const sy = Symbol("y");
console.log("Symbol.valueOf identity:", (Symbol.prototype.valueOf as any).call(sy) === sy);
console.log("BigInt.toString(5n,2):", (BigInt.prototype.toString as any).call(5n, 2));
console.log("BigInt.toString(255n):", (BigInt.prototype.toString as any).call(255n));
console.log("BigInt.valueOf(5n):", (BigInt.prototype.valueOf as any).call(5n) === 5n);

// --- Sanity: the fast direct-call path is unchanged. ---
console.log("direct:", (5).valueOf(), true.toString(), (255n).toString(16), Symbol("z").toString());

// --- Typed `.call`/`.apply` form (no `as any`) must brand-check too. ---
// The statically-typed member `.call` previously folded to `x.<method>()` and
// bypassed the brand-check thunk, returning "[object Object]" instead of
// throwing (the #4100 residual after #4112).
console.log("typed Number.valueOf{}:", threw(() => Number.prototype.valueOf.call({})));
console.log("typed Number.toString{}:", threw(() => Number.prototype.toString.call({})));
console.log("typed Number.toLocaleString{}:", threw(() => Number.prototype.toLocaleString.call({})));
console.log("typed Boolean.valueOf{}:", threw(() => Boolean.prototype.valueOf.call({})));
console.log("typed Boolean.toString{}:", threw(() => Boolean.prototype.toString.call({})));
console.log("typed Number.valueOf.apply{}:", threw(() => Number.prototype.valueOf.apply({})));

// Extracted-local form: `const v = Number.prototype.valueOf; v.call({})`.
const extractedNumValueOf = Number.prototype.valueOf;
const extractedBoolToString = Boolean.prototype.toString;
console.log("extracted Number.valueOf{}:", threw(() => extractedNumValueOf.call({})));
console.log("extracted Boolean.toString{}:", threw(() => extractedBoolToString.call({})));

// Typed form on a valid receiver returns the correct value.
console.log("typed Number.toString(5,2):", Number.prototype.toString.call(5, 2));
console.log("typed Number.valueOf(42):", Number.prototype.valueOf.call(42));
console.log("typed Boolean.toString(true):", Boolean.prototype.toString.call(true));
console.log("extracted Number.valueOf(42):", extractedNumValueOf.call(42));

// `toFixed`/`toExponential` keep folding (must not regress to a false throw).
console.log("typed Number.toFixed(3.14159,2):", Number.prototype.toFixed.call(3.14159, 2));
console.log("typed Number.toExponential(12345,2):", Number.prototype.toExponential.call(12345, 2));
