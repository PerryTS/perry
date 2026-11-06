const key = Symbol("key");
const owner: any = {};
Object.defineProperty(owner, key, { value: 29, writable: false, enumerable: false, configurable: true });
for (let i = 0; i < 30000; i++) {
  const pressure = { i, data: [i, i + 1] };
  if (pressure.i < 0) console.log(pressure);
}
const descriptor = Object.getOwnPropertyDescriptor(owner, key)!;
console.log(owner[key], descriptor.writable, descriptor.enumerable, descriptor.configurable);
console.log(Reflect.set(owner, key, 30), owner[key]);
console.log(Object.getOwnPropertySymbols(owner).length, Object.keys(owner).length);
console.log(delete owner[key], Object.getOwnPropertySymbols(owner).length);
console.log(Object.getOwnPropertyDescriptor({}, key) === undefined);
