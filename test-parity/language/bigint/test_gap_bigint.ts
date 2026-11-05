// BigInt arithmetic + BigInt() constructor parity (closes GH #33).
//
// Before the fix: operators on bigint operands returned NaN (the
// Binary handler always emitted fadd/fsub/fmul/fdiv/frem on the
// NaN-tagged bigint bits), and BigInt(...) failed to compile at all
// ("expression BigIntCoerce not yet supported"). Literals were also
// tagged with POINTER_TAG instead of BIGINT_TAG, so `typeof 5n`
// returned "object".

// --- typeof
console.log(typeof 5n);
console.log(typeof 9223372036854775807n);
const a: bigint = 42n;
console.log(typeof a);

// --- arithmetic on literals
console.log(1n + 2n);
console.log(5n * 3n);
console.log(100n - 50n);
console.log(20n / 4n);
console.log(17n % 5n);

// --- arithmetic on variables
const x: bigint = 5n;
const y: bigint = 3n;
console.log(x + y);
console.log(x * y);
console.log(x - y);

// --- nested / accumulator loop (the @perry/postgres parseBigIntDecimal shape)
function parseBigIntDecimal(s: string): bigint {
    const DIGIT: bigint[] = [0n, 1n, 2n, 3n, 4n, 5n, 6n, 7n, 8n, 9n];
    let n: bigint = 0n;
    for (let i = 0; i < s.length; i++) {
        const code = s.charCodeAt(i) - 48;
        n = n * 10n + DIGIT[code];
    }
    return n;
}
console.log(parseBigIntDecimal('9223372036854775807'));
console.log(parseBigIntDecimal('100000000000000000000'));

// --- BigInt() constructor — number, string, bigint-pass-through
console.log(BigInt(42));
console.log(BigInt('9999999999999999999'));
console.log(BigInt(0));
console.log(BigInt(5n));

// --- comparisons
console.log(10n > 5n);
console.log(10n === 10n);
console.log(10n < 5n);
console.log(100n >= 100n);
