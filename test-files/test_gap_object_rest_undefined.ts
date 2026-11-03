// #12723: CopyDataProperties tests presence independently of the value.
function show(label: string, source: any) {
  const { omit, ...rest } = source;
  console.log(label, Object.keys(rest).join(','), Object.hasOwn(rest, 'u'), rest.u === undefined);
}
show('literal', { omit: 1, u: undefined, n: null, zero: 0 });
const dynamic: any = { omit: 1 };
dynamic.u = undefined;
dynamic.deleted = undefined;
dynamic.tail = 42;
delete dynamic.deleted;
show('dynamic', dynamic);
const { omit: ignored, ...dynamicRest } = dynamic;
console.log('overflow value', dynamicRest.tail);
const inherited: any = Object.create({ inherited: undefined });
inherited.omit = 1;
inherited.u = undefined;
Object.defineProperty(inherited, 'hidden', { value: undefined, enumerable: false });
show('own-only', inherited);
let calls = 0;
show('getter', { omit: 1, get u() { calls++; return undefined; } });
console.log('getter calls', calls);
const key = Symbol('u');
const { omit, ...symbols } = { omit: 1, [key]: undefined };
console.log('symbol', Object.hasOwn(symbols, key), symbols[key] === undefined);
const { 0: first, ...indices } = { 0: 'a', 1: undefined, 2: 'c' };
console.log('indices', Object.keys(indices).join(','), Object.hasOwn(indices, '1'));
