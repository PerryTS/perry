// Deterministic double-to-JSON oracle. Run unchanged in Node and Bun.
// Bits are emitted alongside text so the Rust check bypasses decimal parsing.
import { writeSync } from 'node:fs';

const storage = new ArrayBuffer(8);
const view = new DataView(storage);
let pending = '';
function emitBits(hi, lo) {
    view.setUint32(0, lo, true);
    view.setUint32(4, hi, true);
    const value = view.getFloat64(0, true);
    if (!Number.isFinite(value)) return;
    pending += hi.toString(16).padStart(8, '0') + lo.toString(16).padStart(8, '0')
        + '\t' + JSON.stringify(value) + '\n';
    if (pending.length >= 65536) {
        writeSync(1, pending);
        pending = '';
    }
}
function neighbors(value) {
    view.setFloat64(0, value, true);
    const lo = view.getUint32(0, true);
    const hi = view.getUint32(4, true);
    for (let delta = -8; delta <= 8; delta++) {
        const low = lo + delta;
        const high = (hi + (low < 0 ? -1 : low > 0xffffffff ? 1 : 0)) >>> 0;
        emitBits(high, low >>> 0);
        emitBits((high ^ 0x80000000) >>> 0, low >>> 0);
    }
}
// Every binary exponent, extremes of its significand, and both signs.
for (let exponent = 0; exponent < 2047; exponent++) {
    for (const [high, low] of [[0, 0], [0, 1], [0, 2], [0x7ffff, 0xffffffff],
        [0x80000, 0], [0xfffff, 0xfffffffe], [0xfffff, 0xffffffff]]) {
        emitBits(((exponent << 20) | high) >>> 0, low);
        emitBits((0x80000000 | (exponent << 20) | high) >>> 0, low);
    }
}
// Decimal notation transitions and exact-integer/rounding boundaries.
// Decimal conversion is correctly rounded; Math.pow can differ by one ULP
// between engines, which would feed different inputs to the two oracles.
for (let exponent = -323; exponent <= 308; exponent++) neighbors(Number('1e' + exponent));
for (const value of [0, 1, 0.1, 0.3, 1 / 3, Math.PI, Number.EPSILON,
    1e-6, 1e21, 2 ** 31, 2 ** 32, 2 ** 53, 2 ** 58,
    1000000000000000100, Number.MIN_VALUE, Number.MAX_VALUE]) neighbors(value);
let state = 0x13579bdf;
function randomU32() {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    return state >>> 0;
}
const count = Number(process.argv[2] ?? 1000000);
for (let i = 0; i < count; i++) emitBits(randomU32(), randomU32());
if (pending) writeSync(1, pending);
