const obj: any = {};
Object.defineProperty(obj, "secret", { get() { return 42; }, enumerable: true });
console.log("getter object:", obj);
const prox = new Proxy({ a: 1 }, { get(target, prop, receiver) { return Reflect.get(target, prop, receiver); } });
console.log("proxy:", prox);
