// #3662 — typed-array constructors must validate their length argument with
// ToIndex semantics and throw a `RangeError: Invalid typed array length: <n>`
// on a negative / out-of-range length, instead of silently clamping to 0.
//
// Node truncates toward zero (`NaN` -> 0, `2.5` -> 2, no throw) and throws a
// plain RangeError (no `.code`) on a negative, `Infinity`, or `>= 2**53`
// length. We print the error message so the output is byte-identical to Node.

function r(fn: () => void): string {
    try {
        fn();
        return "NO_THROW";
    } catch (e: any) {
        return `${e.constructor.name}: ${e.message}`;
    }
}

// Negative literal lengths across the typed-array family.
console.log("Uint8Array(-1):", r(() => new Uint8Array(-1)));
console.log("Int8Array(-5):", r(() => new Int8Array(-5)));
console.log("Uint16Array(-2):", r(() => new Uint16Array(-2)));
console.log("Int32Array(-1):", r(() => new Int32Array(-1)));
console.log("Float32Array(-3):", r(() => new Float32Array(-3)));
console.log("Float64Array(-4):", r(() => new Float64Array(-4)));
console.log("Uint8ClampedArray(-1):", r(() => new Uint8ClampedArray(-1)));

// Non-finite / too-large lengths.
console.log("Float64Array(Inf):", r(() => new Float64Array(Infinity)));
console.log("Uint8Array(2**53):", r(() => new Uint8Array(2 ** 53)));

// Negative via a variable (runtime-dispatched path).
const neg = -3;
console.log("Uint8Array(neg):", r(() => new Uint8Array(neg)));
console.log("Int16Array(neg):", r(() => new Int16Array(neg)));

// Valid lengths still allocate; NaN -> 0 and fractional truncates (no throw).
console.log("ok:", new Uint8Array(4).length, new Int32Array(3).length, new Float64Array(0).length);
console.log("NaN->", new Uint8Array(NaN).length, "2.5->", new Uint8Array(2.5).length);
