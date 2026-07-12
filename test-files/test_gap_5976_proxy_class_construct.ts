// #5976: `new W(...)` where `W` is a `Proxy(<class>)` reached the runtime's
// dynamic construct path (`js_new_function_construct`), which probed the value
// as a possible bound-function closure BEFORE its Proxy branch. The probe
// dereferenced `*(proxy_id + 12)` — a small-handle id, not a heap pointer —
// and SIGSEGV'd. Every step below must run and the program must exit 0.

function show(label: string, fn: () => unknown) {
  try {
    console.log(label, "ok", JSON.stringify(fn()));
  } catch (e: any) {
    console.log(label, "throw", e?.constructor?.name);
  }
}

// A proxy whose construct trap returns a non-object → TypeError (per spec).
const badReturn: any = new Proxy(function BadReturn() {}, {
  construct() {
    return 1 as any;
  },
});
show("badret", () => Reflect.construct(badReturn, []));

class Thing {
  value: string;
  constructor(v: string) {
    this.value = v;
  }
  method() {
    return "m:" + this.value;
  }
}

// The crashing shape: construct through a Proxy(class) with an empty handler,
// then use the instance (both `instanceof` and a method call).
show("proxy class empty handler", () => {
  const W: any = new Proxy(Thing, {});
  const i: any = new W("x");
  return [i instanceof Thing, i.method()];
});

// Same, but with a `construct` trap that delegates via Reflect.construct.
const delegating: any = new Proxy(Thing, {
  construct(target: any, args: any[], newTarget: any) {
    const inst: any = Reflect.construct(target, args, newTarget);
    inst.value = `${inst.value}:proxy`;
    return inst;
  },
});
show("proxy class construct trap delegates", () => {
  const i: any = new delegating("y");
  return [i instanceof Thing, i.value, i.method()];
});

// A callable (non-class) proxy target on the same dynamic path.
function Point(this: any, x: number) {
  this.x = x;
}
const PointProxy: any = new Proxy(Point, {});
show("proxy function target", () => {
  const p: any = new PointProxy(3);
  return [p.x, p instanceof Point];
});

// A proxy whose target is itself a proxy of the class (nested forwarding).
const Nested: any = new Proxy(new Proxy(Thing, {}), {});
show("nested proxy class", () => {
  const i: any = new Nested("n");
  return [i instanceof Thing, i.method()];
});

// Calling (not constructing) a proxy-wrapped function goes through the same
// closure-validation gate.
const CallProxy: any = new Proxy(function add(a: number, b: number) {
  return a + b;
}, {});
show("proxy apply", () => CallProxy(2, 3));

// A revoked proxy must throw, not crash.
const rev = Proxy.revocable(Thing, {});
rev.revoke();
show("revoked proxy construct", () => new (rev.proxy as any)("z"));

console.log("END");
