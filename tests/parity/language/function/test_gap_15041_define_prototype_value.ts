// A define of a function's `prototype` that keeps its birth attributes
// (writable, not enumerable, not configurable) stores the value every read sees.

const plain: any = function () {};
Object.defineProperty(plain, "prototype", { value: 5 });
console.log(plain.prototype, plain["proto" + "type"], Reflect.get(plain, "prototype"));

const full: any = function () {};
Object.defineProperty(full, "prototype", {
  value: null,
  writable: true,
  enumerable: false,
  configurable: false,
});
console.log(full.prototype);

const withBag: any = function () {};
withBag.extra = 1;
Reflect.defineProperty(withBag, "prototype", { value: "p" });
console.log(withBag.prototype, Object.getOwnPropertyNames(withBag).join(","));

// [[Set]] through a trap-less Proxy defines the existing own data property.
const target = function () {};
const proxy = new Proxy(new Proxy(target, {}), { set: undefined });
console.log(Reflect.set(proxy, "prototype", null), target.prototype);

const readOnly: any = function () {};
Object.defineProperty(readOnly, "prototype", { value: 7, writable: false });
console.log(readOnly.prototype, Object.getOwnPropertyDescriptor(readOnly, "prototype")!.writable);
