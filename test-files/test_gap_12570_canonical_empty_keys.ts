class EmptyA {}
class EmptyB {}
class EmptyChild extends EmptyA {}
for (const C of [EmptyA, EmptyB, EmptyChild]) {
  const a: any = new C();
  const b: any = new C();
  a.x = 11;
  b.y = 22;
  console.log(Object.keys(a).join(','), Object.keys(b).join(','), a.x, b.y);
  Object.defineProperty(a, 'x', { writable: false, enumerable: false });
  console.log(Object.keys(a).length, Object.getOwnPropertyDescriptor(a, 'x')?.writable);
  console.log(Object.getOwnPropertyNames(a).join(','), Object.keys({ ...b }).join(','));
  delete b.y;
  b.x = 33;
  console.log(Object.keys(b).join(','), b.x);
}
const protoA = { tag: 'a' };
const protoB = { tag: 'b' };
for (const proto of [protoA, protoB]) {
  const o = Object.create(proto);
  Object.defineProperty(o, 'same', { value: 7, enumerable: true, configurable: true });
  console.log(o.tag, o.same, Object.keys(o).join(','));
}
const symbolA = Symbol('same');
const symbolB = Symbol('same');
const symbolObject: any = {};
symbolObject[symbolA] = 1;
symbolObject[symbolB] = 2;
console.log(Object.getOwnPropertySymbols(symbolObject).length, symbolObject[symbolA], symbolObject[symbolB]);
class JsonPrefix { name: string = 'class'; tail: number = 9; }
console.log(Object.keys(new JsonPrefix()).join(','));
for (let i = 0; i < 4; i++) {
  const parsed = JSON.parse('{"name":"json"}');
  console.log(Object.keys(parsed).join(','), parsed.name, parsed.tail, JSON.stringify(parsed));
  parsed.tail = i;
  console.log(Object.keys(parsed).join(','), JSON.stringify(parsed));
}
