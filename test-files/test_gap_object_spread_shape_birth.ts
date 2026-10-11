// Shared spread birth must preserve CopyDataProperties on every refusal.
const base = { x: 3, y: 5, z: 7 };
const a = { before: 1, shared: 2 };
const b = { shared: 3, after: 4 };
for (let i = 0; i < 3; i++) {
  const result = { ...a, ...b, shared: i };
  console.log(Object.keys(result).join(','), result.before, result.shared, result.after);
}
const frozen = Object.freeze({ x: 3, y: 5 });
const thawed = { ...frozen, x: 9 };
thawed.y = 11;
console.log(thawed.x, thawed.y, frozen.x, frozen.y);
console.log(Object.getOwnPropertyDescriptor(thawed, 'y')!.writable);

const hidden: any = { visible: 1 };
Object.defineProperty(hidden, 'hidden', { value: 2, enumerable: false });
console.log(Object.keys({ ...hidden }).join(','));
const sym = Symbol('spread');
const symbols: any = { text: 3 };
symbols[sym] = 5;
const symbolic: any = { ...symbols };
console.log(symbolic.text, symbolic[sym]);

let calls = 0;
const getters = { get x() { calls++; return 7; }, y: 11 };
const read = { ...getters, tail: 13 };
console.log(read.x, read.y, read.tail, calls);
const log: string[] = [];
const proxy = new Proxy(base, {
  ownKeys(target) { log.push('keys'); return Reflect.ownKeys(target); },
  getOwnPropertyDescriptor(target, key) { log.push('descriptor:' + String(key)); return Reflect.getOwnPropertyDescriptor(target, key); },
  get(target, key) { log.push('get:' + String(key)); return Reflect.get(target, key); }
});
const proxied = { ...proxy };
console.log(proxied.x, proxied.y, proxied.z, log.join(','));
console.log(JSON.stringify({ ...'ab', ...[7, 11], ...null, ...undefined }));

Object.defineProperty(Object.prototype, 'spreadInherited', {
  get() { throw new Error('spread must define its own key'); },
  configurable: true
});
const own: any = {};
Object.defineProperty(own, 'spreadInherited', { value: 17, enumerable: true });
const copied: any = { ...own, spreadInherited: 19 };
console.log(copied.spreadInherited);
delete (Object.prototype as any).spreadInherited;

const wide: any = {};
for (let i = 0; i < 40; i++) wide['k' + i] = { value: i };
let checksum = 0;
for (let i = 0; i < 1000; i++) {
  const clone = { ...wide, first: i };
  checksum += clone.k39.value + clone.first;
}
console.log(checksum, Object.keys({ ...wide }).length);

// Ordinary pointer slots exercise barriers and relocation on the shared birth.
const pointerSource = { left: { value: 3 }, right: { value: 5 }, tail: { value: 7 } };
let pointerSum = 0;
let lastPointer: any;
for (let i = 0; i < 10000; i++) {
  lastPointer = { ...pointerSource, left: { value: i } };
  pointerSum += lastPointer.left.value + lastPointer.right.value + lastPointer.tail.value;
}
console.log(pointerSum, lastPointer.left.value, pointerSource.left.value);

// Generalizing a Number lane must retire the learned pointer-free copy.
const evolving: any = { x: 3, y: 5 };
const numericFirst: any = { ...evolving };
const numericSecond: any = { ...evolving };
evolving.x = { value: 17 };
const generalized: any = { ...evolving };
const churned: any[] = [];
for (let i = 0; i < 2000; i++) churned.push({ i, payload: { value: i } });
console.log(generalized.x.value, generalized.y, numericFirst.x, numericSecond.x, churned.length);
let evolvingReads = 0;
Object.defineProperty(evolving, 'x', {
  enumerable: true,
  get() { evolvingReads++; return 29; }
});
console.log(({ ...evolving }).x, evolvingReads);
