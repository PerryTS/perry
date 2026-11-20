// parity-node-argv: --expose-gc
function scan(bytes: Uint8Array, i: any, short: number, mutate: any): any {
  for (; i < short; i++) {
    const c = bytes[i];
    mutate(i, bytes, c);
    if (c === 34) return i + 1;
    if (c === 92) i++;
    if (typeof c === 'object') return c.value;
  }
  return -1;
}
const none = () => {};
const a = new Uint8Array([97, 92, 34, 98, 34]);
for (const i of [0, 1, -1, -0, NaN, 1.5, Infinity, '0']) console.log('index', scan(a, i, 7, none));
console.log('fake', scan(({0: {value: 'kept'}, length: 1} as any), 0, 2, () => { (globalThis as any).gc(); }));
const rab = new ArrayBuffer(5, {maxByteLength: 8});
const v = new Uint8Array(rab);v.set([97,98,99,34,0]);
console.log('resize', scan(v, 0, 5, (i: number) => { if (i === 1) rab.resize(2); }));
const b = new ArrayBuffer(5); const w = new Uint8Array(b);w.set([97,98,99,34,0]);
console.log('detach', scan(w, 0, 5, (i: number) => { if (i === 1) structuredClone(b, {transfer: [b]}); }));
const shifted = new Uint8Array(new Uint8Array([0,97,98,34,0]).buffer, 1, 3);
console.log('offset', scan(shifted, 0, 5, () => { (globalThis as any).gc(); }));
const expando: any = new Uint8Array(2); expando.note = {value: 'property'};
const key: any = { valueOf() { return 0; }, toString() { return 'note'; } };
console.log('boxed-key', scan(expando, key, 1, none));

function pure(bytes: Uint8Array, i: number, short: number): any {
  for (; i < short; i++) {
    const c = bytes[i];
    if (c === 34) return i + 1;
    if (c === 92) i++;
    if (typeof c === 'object') return c;
  }
  return -1;
}
for (const i of [0, 1, -1, NaN, 1.5, Infinity, '0', false, true]) console.log('pure', pure(a, i as any, 7));
const pureFake = pure(({0: {value: 'rooted'}} as any), 0, 2);
console.log('pure-fake', pureFake.value);
console.log('pure-offset', pure(shifted, 0, 5));

function pureMutating(bytes: Uint8Array, i: number, short: number, mutate: any): any {
  for (; i < short; i++) {
    const c = bytes[i];
    mutate(i);
    if (c === 34) return i + 1;
    if (typeof c === 'object') return c;
  }
  return -1;
}
const r2 = new ArrayBuffer(5, {maxByteLength: 8});
const v2 = new Uint8Array(r2); v2.set([97,98,99,34,0]);
console.log('pure-resize', pureMutating(v2, 0, 5, (i: number) => { if (i === 1) r2.resize(2); }));
const b2 = new ArrayBuffer(5);
const w2 = new Uint8Array(b2); w2.set([97,98,99,34,0]);
console.log('pure-detach', pureMutating(w2, 0, 5, (i: number) => { if (i === 1) structuredClone(b2, {transfer: [b2]}); }));
console.log('pure-gc', pureMutating(shifted, 0, 5, () => { (globalThis as any).gc(); }));
const rooted = pureMutating(({0: {value: 'survives-gc'}} as any), 0, 2, () => { (globalThis as any).gc(); });
console.log('pure-fake-gc', rooted.value);

function equality(bytes: Uint8Array, i: number, short: number): number {
  let mask = 0;
  for (; i < short; i++) {
    const c = bytes[i];
    if (c === undefined) mask |= 1;
    if (c === c) mask |= 2;
    if (c === 0) mask |= 4;
    if (c !== 0) mask |= 8;
    if (c === NaN) mask |= 16;
    if (c !== NaN) mask |= 32;
    if (-0 === c) mask |= 64;
  }
  return mask;
}
for (const i of [0, -1, -0, NaN, 1.5, '0', false]) {
  console.log('equality', equality(new Uint8Array([0]), i as any, 3));
}
console.log('equality-fake', equality(({0: {value: 'heap'}} as any), 0, 1));

let changing: Uint8Array = new Uint8Array([97,98,34]);
function changingReceiver(i: number, short: number, mutate: any): any {
  for (; i < short; i++) {
    const c = changing[i];
    mutate(i);
    if (typeof c === 'object') return c;
  }
  return undefined;
}
const changed = changingReceiver(0, 3, (i: number) => {
  if (i === 0) changing = ({1: {value: 'swapped'}} as any);
  (globalThis as any).gc();
});
console.log('changing-receiver', changed.value);
const table: any = new Uint8Array([1]);
table.undefined = {value: 'undefined-key'};
function nullableKey(bytes: Uint8Array, i: number, short: number, target: Uint8Array, mutate: any): any {
  for (; i < short; i++) {
    const key = bytes[i];
    const c = target[key];
    mutate();
    if (typeof c === 'object') return c;
  }
  return undefined;
}
const missingKey = nullableKey(new Uint8Array([0]), 0, 3, table, () => { (globalThis as any).gc(); });
console.log('nullable-key', missingKey.value);
