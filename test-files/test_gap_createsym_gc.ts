const sym = Symbol("kept");
const held: any[] = [];
for (let i = 0; i < 120; i++) {
  const proto: any = { inherited: i };
  const bag: any = { value: { value: { n: i }, enumerable: true } };
  bag[sym] = { value: { n: i + 1 }, enumerable: true };
  const obj: any = Object.create(proto, bag);
  const desc: any = Object.getOwnPropertyDescriptors(obj);
  const clone: any = Object.create(proto, desc);
  held.push([proto, obj, clone, Object.assign(Object.create(proto), obj)]);
}
let total = 0;
for (const row of held) {
  for (let k = 1; k < 4; k++) {
    const o = row[k];
    if (Object.getPrototypeOf(o) !== row[0] || o.value !== row[1].value || o[sym] !== row[1][sym])
      throw new Error("lost reference");
    total += o.value.n + o[sym].n + o.inherited;
  }
}
console.log("retained", held.length, total);
