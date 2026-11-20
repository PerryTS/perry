const sym = Symbol.for('owned-symbol');
for (let i = 0; i < 40000; i++) { const a = { i, nested: [i, i + 1] }; if (i === 39999) console.log(a.i); }
console.log(sym.description);
console.log(Symbol.keyFor(sym));
console.log(Symbol.for('owned-symbol') === sym);
console.log(Symbol.iterator.description);
console.log(Symbol.keyFor(Symbol.iterator));
console.log(Symbol.keyFor(Symbol()));
