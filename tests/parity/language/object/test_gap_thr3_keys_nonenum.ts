// THR3: a non-enumerable constructor-defined key is skipped by Object.keys,
// for-in, JSON.stringify and spread, and listed by getOwnPropertyNames at
// its definition position.
class P {
  a: number;
  constructor() {
    this.a = 1;
    Object.defineProperty(this, "hid", { value: 2, writable: true });
    (this as any).b = 3;
    Object.defineProperties(this, {
      pos: { configurable: true, enumerable: true, value: { x: 1 } },
      rot: { value: 5 },
    });
    (this as any).c = 4;
  }
}
const p = new P();
const forin: string[] = [];
for (const k in p) forin.push(k);
console.log(Object.keys(p).join(","));
console.log(forin.join(","));
console.log(JSON.stringify(p));
console.log(Object.keys({ ...p }).join(","));
console.log(Object.getOwnPropertyNames(p).join(","));
console.log(Reflect.ownKeys(p).join(","));
