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
