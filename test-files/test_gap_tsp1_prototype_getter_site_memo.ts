function opaque(value: any) {
  const box = new Map();
  box.set('value', value);
  return box.get('value');
}
let calls = 0;
class Base {
  value = 2;
  get width() { calls++; return this.value; }
}
class Child extends Base { marker = 1; }
function read(o: any) { return o.width; }
const child: any = opaque(new Child());
let sum = 0;
for (let i = 0; i < 100; i++) sum += read(child);
console.log('warm', sum, calls);
Object.defineProperty(Base.prototype, 'width', {
  get() { calls++; return this.value + 10; }, configurable: true,
});
console.log('redefined', read(child), calls);
const other = { get width() { calls++; return this.value + 20; } };
Object.setPrototypeOf(child, other);
console.log('swapped', read(child), calls);
Object.defineProperty(child, 'width', { value: 99, configurable: true });
console.log('own', read(child), calls);
delete (child as any).width;
console.log('deleted-own', read(child), calls);
const frozen = Object.freeze({ get width() { calls++; return this.value + 30; } });
const cold: any = opaque(Object.create(frozen));
cold.value = 4;
for (let i = 0; i < 100; i++) read(cold);
console.log('frozen', read(cold), calls);
const top = { get width() { calls++; return this.value + 40; } };
const middle = Object.create(top);
const deep: any = opaque(Object.create(middle));
deep.value = 5;
for (let i = 0; i < 100; i++) read(deep);
console.log('deep', read(deep), calls);
Object.defineProperty(middle, 'width', { value: 77, configurable: true });
console.log('middle-shadow', read(deep), calls);
delete middle.width;
let traps = 0;
const proxy = new Proxy(top, {
  get(target, key, receiver) {
    traps++;
    return Reflect.get(target, key, receiver);
  },
});
Object.setPrototypeOf(middle, proxy);
console.log('proxy', read(deep), traps, calls);
const signal: any = opaque({ get width() { calls++; return calls; } });
console.log('effects', read(signal), read(signal), read(signal));

function keyed(o: any, key: string) { return o[key]; }
const keyedProto = { get a() { return 2; }, b: 4 };
const keyedObject: any = opaque(Object.create(keyedProto));
const names = ['a', 'b'];
function keyedSum(o: any) {
  let total = 0;
  for (let i = 0; i < 100; i++) total += keyed(o, names[i & 1]);
  return total;
}
console.log('keyed-warm', keyedSum(keyedObject));
Object.defineProperty(keyedProto, 'a', { get() { return 10; }, configurable: true });
console.log('keyed-redefined', keyedSum(keyedObject));
Object.defineProperty(keyedObject, 'a', { value: 20, configurable: true });
console.log('keyed-own', keyedSum(keyedObject));
