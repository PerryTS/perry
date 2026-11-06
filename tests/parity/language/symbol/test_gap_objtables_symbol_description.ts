const sym = Symbol('owned-symbol');
const obj = {[sym]: 37};
for (let i = 0; i < 40000; i++) { const a = { i, nested: [i, i + 1] }; if (i === 39999) console.log(a.i); }
console.log(sym.description);
console.log(String(sym));
console.log(obj[sym]);
console.log(Object.getOwnPropertySymbols(obj)[0] === sym);
console.log(Symbol().description);
