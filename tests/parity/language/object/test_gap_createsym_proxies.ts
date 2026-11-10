const sym = Symbol("proxy");
const log: string[] = [];
const target: any = { a: 1 }; target[sym] = 2;
const source: any = new Proxy(target, {
  ownKeys(t) { log.push("keys"); return Reflect.ownKeys(t); },
  getOwnPropertyDescriptor(t, k) { log.push("desc " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
  get(t, k, r) { log.push("get " + String(k)); return Reflect.get(t, k, r); }
});
console.log("symbols", Object.getOwnPropertySymbols(source).map(String).join("|"), log.join("|")); log.length = 0;
console.log("keys", Reflect.ownKeys(source).map(String).join("|"), log.join("|")); log.length = 0;
Object.getOwnPropertyDescriptors(source);
console.log("descriptors", log.join("|")); log.length = 0;
Object.assign({}, source);
console.log("assign", log.join("|")); log.length = 0;
const descriptorTarget: any = { a: { value: 3, enumerable: true } };
descriptorTarget[sym] = { value: 4, enumerable: true };
const descriptorProxy: any = new Proxy(descriptorTarget, {
  ownKeys(t) { log.push("keys"); return Reflect.ownKeys(t); },
  getOwnPropertyDescriptor(t, k) { log.push("desc " + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
  get(t, k, r) { log.push("get " + String(k)); return Reflect.get(t, k, r); }
});
const created: any = Object.create(source, descriptorProxy);
console.log("create", Object.getPrototypeOf(created) === source, created.a, created[sym], log.join("|"));
