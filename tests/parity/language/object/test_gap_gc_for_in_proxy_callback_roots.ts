// for...in retains its output and current receiver while Proxy traps run JS.
// The inherited proxy fires after own keys have already grown the output past
// its initial capacity. Churn in each trap exposes stale locals under the GC
// schedule/protection matrix without relying on a Node-only gc() function.
let trapCalls = 0;
function churn() {
  for (let i = 0; i < 24; i++) {
    const garbage = { value: ["temporary", i, trapCalls] };
    if (garbage.value.length !== 3) throw new Error("allocation witness");
  }
  trapCalls++;
}
function wrapped(target: any): any {
  return new Proxy(target, {
    ownKeys(value) { churn(); return Reflect.ownKeys(value); },
    getOwnPropertyDescriptor(value, key) {
      churn(); return Reflect.getOwnPropertyDescriptor(value, key);
    },
    getPrototypeOf(value) { churn(); return Reflect.getPrototypeOf(value); },
  });
}
const inherited = wrapped({ inheritedA: 1, inheritedB: 2, hidden: 3 });
const target: any = Object.create(inherited);
for (let i = 0; i < 14; i++) target["own" + i] = i;
Object.defineProperty(target, "hidden", { value: 4, enumerable: false, configurable: true });
for (const receiver of [target, wrapped(target)]) {
  const keys: string[] = [];
  for (const key in receiver) keys.push(key);
  const expected = Array.from({ length: 14 }, (_, i) => "own" + i).concat(["inheritedA", "inheritedB"]);
  if (keys.join(",") !== expected.join(",")) throw new Error("enumeration lost keys: " + keys.join(","));
  console.log(keys.join(","));
}
if (trapCalls === 0) throw new Error("traps were not invoked");
const descriptorProxy = new Proxy({ property_name: 23 }, {
  getOwnPropertyDescriptor(value, key) {
    return {
      enumerable: true, configurable: true, writable: true,
      get value() { churn(); return value[key]; },
    };
  },
});
const descriptor = Reflect.getOwnPropertyDescriptor(descriptorProxy, "property_name")!;
if (descriptor.value !== 23 || !descriptor.writable || !descriptor.enumerable || !descriptor.configurable) {
  throw new Error("descriptor fields lost across collection");
}
console.log("PASS for-in callback roots");
