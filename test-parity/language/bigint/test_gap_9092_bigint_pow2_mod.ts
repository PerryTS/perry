// BigInt remainder fast paths (power-of-two divisor, single-limb divisor)
// must preserve exact ECMA BigInt::remainder semantics: sign follows the
// DIVIDEND, canonical zero (no -0n), and non-power-of-two divisors are
// unchanged. Motivated by TypeBox Value.Hash's FNV-1a `% 2^64` idiom, which
// previously went through full 1024-bit binary long division per byte.

// Sign follows dividend.
console.log("a:", ((-7n) % 4n).toString());            // -3
console.log("b:", (7n % -4n).toString());              // 3
console.log("c:", ((-7n) % -4n).toString());           // -3

// Exact multiple of a power of two: canonical zero, never -0n.
const z = (-8n) % 8n;
console.log("d:", z.toString(), z === 0n);             // 0 true
const z2 = (-(2n ** 100n)) % (2n ** 64n);
console.log("e:", z2.toString(), z2 === 0n);           // 0 true

// Dividend smaller than the divisor: unchanged (both signs).
console.log("f:", (5n % (2n ** 64n)).toString());      // 5
console.log("g:", ((-5n) % (2n ** 64n)).toString());   // -5
console.log("h:", (123456789n % (2n ** 200n)).toString());

// Dividend === divisor.
console.log("i:", ((2n ** 64n) % (2n ** 64n)).toString());   // 0
console.log("j:", ((2n ** 64n) / (2n ** 64n)).toString());   // 1

// Huge (multi-limb, 200+ bit) dividends.
const huge = 2n ** 250n + 2n ** 129n + 2n ** 64n + 987654321987654321n;
console.log("k:", (huge % (2n ** 64n)).toString());
console.log("l:", (huge % (2n ** 13n)).toString());
console.log("m:", ((-huge) % (2n ** 64n)).toString());
console.log("n:", ((-huge) % (2n ** 13n)).toString());
console.log("o:", (huge / (2n ** 64n)).toString());
console.log("p:", ((-huge) / (2n ** 64n)).toString());
console.log("q:", (huge % (2n ** 70n)).toString());    // non-limb-aligned pow2

// Modulo 1n is always 0n; division by 1n is identity.
console.log("r:", (huge % 1n).toString(), ((-huge) % 1n).toString());
console.log("s:", (huge / 1n).toString());

// Single-limb non-power-of-two divisors (FNV prime).
const prime = 1099511628211n;
console.log("t:", ((2n ** 64n) % prime).toString());
console.log("u:", (huge % prime).toString());
console.log("v:", ((-huge) % prime).toString());
console.log("w:", (huge / prime).toString());

// Multi-limb non-power-of-two divisors take the general path, unchanged.
console.log("x:", (huge % (2n ** 64n + 1n)).toString());
console.log("y:", (huge % (2n ** 64n + 3n)).toString());
console.log("z:", (huge / (2n ** 64n + 1n)).toString());
console.log("A:", ((-huge) % (2n ** 64n + 1n)).toString());

// Division by zero still throws RangeError before any fast path.
try {
  console.log((huge % 0n).toString());
} catch (e) {
  console.log("B:", e instanceof RangeError);
}

// The FNV-1a 64 loop itself (TypeBox Value.Hash idiom) — checksum must
// match node bit-for-bit.
const Prime = BigInt("1099511628211");
const Size = BigInt("18446744073709551616");
const Bytes = Array.from({ length: 256 }, (_, i) => BigInt(i));
let Accumulator = BigInt("14695981039346656037");
let s32 = 0x12345678;
for (let i = 0; i < 4096; i++) {
  s32 ^= (s32 << 13); s32 >>>= 0;
  s32 ^= (s32 >>> 17);
  s32 ^= (s32 << 5); s32 >>>= 0;
  Accumulator = Accumulator ^ Bytes[s32 & 0xff];
  Accumulator = (Accumulator * Prime) % Size;
}
console.log("C:", Accumulator.toString());

// Negative power-of-two DIVISOR (magnitude taken via two's-complement
// negation before the fast path; result sign still follows the dividend).
const p64 = 2n ** 64n;
console.log("D:", (huge % -p64).toString());
console.log("E:", ((-huge) % -p64).toString());
console.log("F:", (huge / -p64).toString());
console.log("G:", ((-huge) / -p64).toString());

// Zero dividend on every tier.
console.log("H:", (0n % p64).toString(), (0n / p64).toString());
console.log("I:", (0n % prime).toString(), (0n % (p64 + 1n)).toString());

// Dividend exactly one below a power-of-two divisor.
console.log("J:", ((p64 - 1n) % p64).toString(), ((p64 - 1n) / p64).toString());

// Dividend === divisor on the non-power-of-two tiers.
console.log("K:", (prime % prime).toString(), (prime / prime).toString());
console.log("L:", ((p64 + 1n) % (p64 + 1n)).toString());

// Very wide power-of-two divisor (bit 1021) with a 1022-bit dividend.
const wide = 2n ** 1022n + 12345n;
console.log("M:", (wide % (2n ** 1021n)).toString());
console.log("N:", (wide / (2n ** 1021n)).toString());
console.log("O:", ((-wide) % (2n ** 1021n)).toString());

// Power-of-two divisors across the limb boundary, both signs of dividend.
for (const k of [1n, 7n, 63n, 64n, 65n, 127n, 128n, 129n, 200n]) {
  const d = 2n ** k;
  console.log("P" + k + ":", (huge % d).toString(), ((-huge) % d).toString(), (huge / d).toString());
}

// Identity check a === (a / b) * b + (a % b) across mixed signs and tiers.
// Two shapes deliberately avoided here because they trip pre-existing perry
// lowering gaps that have nothing to do with division: `-arr[i]` on a BigInt
// above 2^53 float-negates, and inlining `(a / d) * d + (a % d)` on for-of
// bound BigInts drops the remainder term. Quotient and remainder are bound to
// locals first, and every negation is applied to a named local.
const d64 = 2n ** 64n;
const d13 = 2n ** 13n;
const dWide = p64 + 1n;
const nhuge = -huge;
const divisors = [
  d64, -d64, d13, -d13, prime, -prime, dWide, -dWide, 3n, -3n, 1n, -1n,
];
const dividends = [huge, nhuge, 12345n, -12345n, 0n, d64, -d64, d64 - 1n];
let identityOk = true;
let remainderInRange = true;
let signOk = true;
for (const d of divisors) {
  for (const a of dividends) {
    const q = a / d;
    const r = a % d;
    if (q * d + r !== a) identityOk = false;
    // |r| < |d|, expressed without negating an array element.
    const rmag = r < 0n ? 0n - r : r;
    const dmag = d < 0n ? 0n - d : d;
    if (rmag >= dmag) remainderInRange = false;
    // The remainder is either exactly zero or carries the dividend's sign.
    if (r !== 0n && (r < 0n) !== (a < 0n)) signOk = false;
  }
}
console.log("Q:", identityOk, remainderInRange, signOk);
