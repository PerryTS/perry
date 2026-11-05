// Node differential: number spelling in scalar, object, array, pretty,
// replacer and toJSON paths. Include exact halfway and notation boundaries.
const values = [
    0, -0, NaN, Infinity, -Infinity, 5e-324, 2.2250738585072014e-308,
    Number.MAX_VALUE, 1e-7, 1e-6, 1e20, 1e21, 9007199254740991,
    9007199254740992, 288230376151711744, 1000000000000000100,
    0.1, 0.3, 1 / 3, Math.PI, 562949953421312.25, -562949953421312.25,
    1125899906842624.25, 100000000000000.125, -28008981190621.5625,
];
for (let i = 0; i < values.length; i++) {
    const value = values[i];
    console.log(JSON.stringify(value));
    console.log(JSON.stringify({ n: value }));
    console.log(JSON.stringify([value]));
}
console.log(JSON.stringify(562949953421312.25));
console.log(JSON.stringify({ n: 562949953421312.25 }, null, 2));
console.log(JSON.stringify({ n: 1 }, (key, value) => key === 'n' ? 562949953421312.25 : value));
console.log(JSON.stringify({ toJSON() { return 562949953421312.25; } }));
