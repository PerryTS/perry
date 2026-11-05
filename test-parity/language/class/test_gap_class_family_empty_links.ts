// An empty class whose instances get their keys through a lookup function
// (tsc's SymbolLinks / getSymbolLinks form).
class Links {}
class Empty {
  run(n: number): number {
    return n + 1;
  }
}
const table: Links[] = [];
function getLinks(id: number): any {
  return table[id] ?? (table[id] = new Links());
}
let sum = 0;
for (let r = 0; r < 200; r++) {
  for (let i = 0; i < 2000; i++) {
    const l = getLinks(i);
    if (l.type === undefined) {
      l.type = i;
      l.declaredType = i * 2;
      l.target = r;
    }
    l.mapper = r;
    if (i % 3 === 0) l.instantiations = i;
    sum += l.type + l.declaredType + l.mapper;
  }
}
const e = new Empty();
for (let i = 0; i < 1000000; i++) sum = e.run(sum) % 1000000007;
console.log(sum, Object.keys(getLinks(3)).join(","), Object.keys(getLinks(4)).join(","));
