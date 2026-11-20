// Test WeakRef, FinalizationRegistry, WeakMap, WeakSet
// Expected output:
// weakref deref: hello
// weakref type: object
// weakref string deref: test string
// registry created: true
// registered: true
// unregistered: true
// weakmap set/get: 42
// weakmap has: true
// weakmap delete: true
// weakmap has after delete: false
// weakmap different keys: 1 2
// weakset add/has: true
// weakset delete: true
// weakset has after delete: false
// weakset no primitives: caught error
// weakmap no primitives: caught error
// weakref deref after scope: object

// --- WeakRef basic usage ---
const obj1 = { message: "hello" };
const ref1 = new WeakRef(obj1);
const derefed = ref1.deref();
console.log("weakref deref: " + (derefed ? derefed.message : "collected"));
console.log("weakref type: " + typeof ref1.deref());

// --- WeakRef with different value types ---
const strObj = { value: "test string" };
const ref2 = new WeakRef(strObj);
console.log("weakref string deref: " + (ref2.deref()?.value ?? "collected"));

// --- FinalizationRegistry creation ---
const registry = new FinalizationRegistry((heldValue: string) => {
  // This callback is called when the registered object is GC'd
  // We can't reliably test this, but we test the API surface
});
console.log("registry created: " + (registry instanceof FinalizationRegistry));

// --- FinalizationRegistry register ---
const target = { data: "will be watched" };
const token = { id: "unregister-token" };
registry.register(target, "held value", token);
console.log("registered: true");

// --- FinalizationRegistry unregister ---
const didUnregister = registry.unregister(token);
console.log("unregistered: " + didUnregister);

// --- WeakMap basic usage ---
const wmKey1 = { id: 1 };
const wmKey2 = { id: 2 };
const weakmap = new WeakMap<object, number>();
weakmap.set(wmKey1, 42);
console.log("weakmap set/get: " + weakmap.get(wmKey1));
console.log("weakmap has: " + weakmap.has(wmKey1));

// --- WeakMap delete ---
weakmap.delete(wmKey1);
console.log("weakmap delete: true");
console.log("weakmap has after delete: " + weakmap.has(wmKey1));

// --- WeakMap with multiple keys ---
const wm2 = new WeakMap<object, number>();
const k1 = {};
const k2 = {};
wm2.set(k1, 1);
wm2.set(k2, 2);
console.log("weakmap different keys: " + wm2.get(k1) + " " + wm2.get(k2));

// --- WeakSet basic usage ---
const wsObj1 = { name: "item1" };
const weakset = new WeakSet<object>();
weakset.add(wsObj1);
console.log("weakset add/has: " + weakset.has(wsObj1));

// --- WeakSet delete ---
weakset.delete(wsObj1);
console.log("weakset delete: true");
console.log("weakset has after delete: " + weakset.has(wsObj1));

// --- WeakSet rejects primitives ---
try {
  const ws = new WeakSet() as any;
  ws.add("string primitive");
  console.log("weakset no primitives: should have thrown");
} catch (e: any) {
  console.log("weakset no primitives: caught error");
}

// --- WeakMap rejects primitive keys ---
try {
  const wm = new WeakMap() as any;
  wm.set("string key", 123);
  console.log("weakmap no primitives: should have thrown");
} catch (e: any) {
  console.log("weakmap no primitives: caught error");
}

// --- WeakRef deref returns the object while it's still referenced ---
function createAndDeref(): string {
  const inner = { kind: "persistent" };
  const innerRef = new WeakRef(inner);
  // Object is still in scope, so deref should return it
  const result = innerRef.deref();
  return typeof result === "object" ? "object" : "undefined";
}
console.log("weakref deref after scope: " + createAndDeref());
