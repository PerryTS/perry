// ToInt32/ToUint32 for |x| >= 2^63 must truncate-mod-2^32, not saturate.
// Force the dynamic (any-typed) bitwise/shift helpers + Math.clz32. #6079.
const big: any = 1e20;
const neg: any = -1e20;
const b2: any = 5e9;
const b3: any = 3e9;
console.log(Math.clz32(big), Math.clz32(1e40), Math.clz32(0), Math.clz32(-1), Math.clz32(2 ** 32));
console.log(big | 0, neg | 0, big >>> 0);
console.log(big & 0xffff, big | 1, big ^ 0);
console.log(big >> 4, big << 4, big >>> 4);
console.log(b2 | 0, b2 >>> 0, b2 & 7, b3 | 0);
// sanity: small any-typed operands unchanged
const s: any = 5;
console.log(s | 2, s & 3, s ^ 1, s << 1, s >> 1, s >>> 1, ~s);
