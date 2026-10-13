function* values() { yield { n: 17 }; return 'finished'; }
const g = values();
const a = g.next();
const b = g.next();
const c = g.next();
console.log(Object.keys(a).join(','), JSON.stringify(a));
console.log(Object.keys(b).join(','), JSON.stringify(b));
console.log(Object.keys(c).join(','), JSON.stringify(c));
for (const k of Object.keys(a)) {
  const d = Object.getOwnPropertyDescriptor(a, k);
  console.log(k, d.writable, d.enumerable, d.configurable);
}
a.expando = 19;
console.log(a.expando, Object.getPrototypeOf(a) === Object.prototype, a !== b);
