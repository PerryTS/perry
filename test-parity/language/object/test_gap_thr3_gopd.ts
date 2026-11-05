// THR3: descriptors of constructor-defined keys (three.js style).
class Thing {
  constructor(v: number) {
    Object.defineProperty(this, "id", { value: v });
    Object.defineProperty(this, "uid", { configurable: true, enumerable: true, value: v * 2 });
    (this as any).name = "t";
    Object.defineProperty(this, "w", { value: 3, writable: true });
  }
}
const t = new Thing(4);
for (const k of ["id", "uid", "name", "w"]) {
  console.log(k, JSON.stringify(Object.getOwnPropertyDescriptor(t, k)));
}
console.log(JSON.stringify(Object.getOwnPropertyDescriptors(new Thing(1))));
(t as any).w = 8;
console.log((t as any).w, (t as any).uid, (t as any).id);
