function classify(v: any): string {
  return '' + (v < 128) + ',' + (v >= 2048) + ',' + (v <= 55295) + ',' + (v > 57343);
}
for (const v of [0, -0, -1, 1.25, NaN, Infinity, -Infinity, null, undefined, false, true, '', '127', '128', 'x', 127n, 128n, {}, { valueOf() { return 127; } }]) {
  console.log(classify(v));
}
let calls = 0;
const v = { valueOf() { calls++; return calls % 2 === 0 ? 60000 : 0; } };
console.log(classify(v), calls);
try { console.log(classify(Symbol('x'))); } catch (e) { console.log('symbol throws', e instanceof TypeError); }
