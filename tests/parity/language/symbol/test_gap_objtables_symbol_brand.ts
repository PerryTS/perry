const fresh = Symbol('fresh-brand');
const registered = Symbol.for('objtables-brand');
const obj = {[fresh]: 11, [registered]: 13};
for (let i = 0; i < 40000; i++) { const a = {i, nested: [i, i + 1]}; if (i === 39999) console.log(a.i); }
console.log(typeof fresh);
console.log(typeof registered);
console.log(typeof obj);
console.log(obj[fresh] + obj[registered]);
console.log(Symbol.keyFor(fresh));
console.log(Symbol.keyFor(registered));
console.log(Object.getOwnPropertySymbols(obj).length);
