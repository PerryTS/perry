// THR3: redefining a configurable constructor-defined key to non-configurable,
// then a redefine with another value throws; delete returns false; a
// configurable key deletes.
class O {
  constructor() {
    Object.defineProperty(this, "position", { configurable: true, enumerable: true, value: 1 });
    Object.defineProperty(this, "gone", { configurable: true, value: 2 });
  }
  get3(): number {
    return (this as any).position;
  }
}
const o: any = new O();
Object.defineProperty(o, "position", { configurable: false });
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(o, "position")));
try {
  Object.defineProperty(o, "position", { value: 7 });
  console.log("redefined");
} catch (e) {
  console.log((e as Error).constructor.name);
}
console.log(Reflect.deleteProperty(o, "position"), o.position, o.get3());
console.log(delete o.gone, "gone" in o, Object.getOwnPropertyNames(o).join(","));
const o2: any = new O();
Object.defineProperty(o2, "position", { value: 9, writable: true });
o2.position = 10;
console.log(o2.position, o2.get3(), JSON.stringify(Object.getOwnPropertyDescriptor(o2, "position")));
