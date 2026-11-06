// Test Proxy and Reflect metaprogramming
// Expected output:
// intercepted get: greeting
// hello world
// intercepted set: count = 42
// 42
// 42
// has name: true
// has missing: false
// applied with args: 3,4
// 7
// constructed with args: Alice
// Alice
// deleted key: temp
// true
// revokedGet: hello
// proxy revoked
// reflectGet: 10
// reflectSet: true
// 20
// reflectHas: true
// reflectHas missing: false
// reflectOwnKeys: x,y,z
// reflectApply: 6
// reflectConstruct name: Bob
// reflectDefineProperty: true
// descriptor value: 99
// reflectGetPrototypeOf: true

// --- Proxy with get trap ---
const targetObj: Record<string, any> = { greeting: "hello world" };
const getHandler = {
  get(target: Record<string, any>, prop: string): any {
    console.log("intercepted get: " + prop);
    return target[prop];
  }
};
const getProxy = new Proxy(targetObj, getHandler);
console.log(getProxy.greeting);

// --- Proxy with set trap ---
const setTarget: Record<string, any> = {};
const setHandler = {
  set(target: Record<string, any>, prop: string, value: any): boolean {
    console.log("intercepted set: " + prop + " = " + value);
    target[prop] = value;
    return true;
  }
};
const setProxy = new Proxy(setTarget, setHandler);
console.log(setProxy.count = 42);
console.log(setTarget.count);

// --- Proxy with has trap (in operator) ---
const hasTarget: Record<string, any> = { name: "test" };
const hasHandler = {
  has(target: Record<string, any>, prop: string): boolean {
    const result = prop in target;
    console.log("has " + prop + ": " + result);
    return result;
  }
};
const hasProxy = new Proxy(hasTarget, hasHandler);
"name" in hasProxy;
"missing" in hasProxy;

// --- Proxy with apply trap (function proxy) ---
function addNumbers(a: number, b: number): number {
  return a + b;
}
const applyHandler = {
  apply(target: any, thisArg: any, argsList: any[]): any {
    console.log("applied with args: " + argsList);
    return target.apply(thisArg, argsList);
  }
};
const applyProxy = new Proxy(addNumbers, applyHandler);
console.log(applyProxy(3, 4));

// --- Proxy with construct trap ---
class Person {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
const constructHandler = {
  construct(target: any, args: any[]): any {
    console.log("constructed with args: " + args[0]);
    return new target(...args);
  }
};
const constructProxy = new Proxy(Person, constructHandler);
const person = new constructProxy("Alice");
console.log(person.name);

// --- Proxy with deleteProperty trap ---
const deleteTarget: Record<string, any> = { temp: "will be deleted", keep: "stays" };
const deleteHandler = {
  deleteProperty(target: Record<string, any>, prop: string): boolean {
    console.log("deleted key: " + prop);
    delete target[prop];
    return true;
  }
};
const deleteProxy = new Proxy(deleteTarget, deleteHandler);
console.log(delete (deleteProxy as any).temp);

// --- Proxy.revocable ---
const revTarget: Record<string, any> = { hello: "revokedGet: hello" };
const { proxy: revProxy, revoke } = Proxy.revocable(revTarget, {});
console.log(revProxy.hello);
revoke();
try {
  revProxy.hello;
} catch (e: any) {
  console.log("proxy revoked");
}

// --- Reflect.get ---
const reflectObj = { x: 10, y: 20, z: 30 };
console.log("reflectGet: " + Reflect.get(reflectObj, "x"));

// --- Reflect.set ---
const reflectSetObj: Record<string, any> = { x: 10 };
console.log("reflectSet: " + Reflect.set(reflectSetObj, "x", 20));
console.log(reflectSetObj.x);

// --- Reflect.has ---
console.log("reflectHas: " + Reflect.has(reflectObj, "x"));
console.log("reflectHas missing: " + Reflect.has(reflectObj, "w"));

// --- Reflect.ownKeys ---
console.log("reflectOwnKeys: " + Reflect.ownKeys(reflectObj).join(","));

// --- Reflect.apply ---
function multiply(a: number, b: number): number {
  return a * b;
}
console.log("reflectApply: " + Reflect.apply(multiply, null, [2, 3]));

// --- Reflect.construct ---
const bob = Reflect.construct(Person, ["Bob"]);
console.log("reflectConstruct name: " + bob.name);

// --- Reflect.defineProperty ---
const defObj: Record<string, any> = {};
console.log("reflectDefineProperty: " + Reflect.defineProperty(defObj, "val", { value: 99, writable: true }));
console.log("descriptor value: " + defObj.val);

// --- Reflect.getPrototypeOf ---
class Animal {}
class Dog extends Animal {}
const dog = new Dog();
console.log("reflectGetPrototypeOf: " + (Reflect.getPrototypeOf(dog) === Dog.prototype));
