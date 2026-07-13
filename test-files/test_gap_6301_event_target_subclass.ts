// #6301: `class X extends EventTarget` must inherit the EventTarget method
// surface. The methods used to exist only as a compile-time lowering keyed on a
// receiver whose static class name was literally `EventTarget`, so nothing was
// readable as a value (`typeof t.addEventListener` was `undefined` even on a
// plain `new EventTarget()`) and a subclass inherited nothing at all —
// `this.dispatchEvent(...)` threw `TypeError: value is not a function`. That is
// the root cause of #5931 (cac v7's `class CAC extends EventTarget`).

// ── plain EventTarget: methods are readable as values ──
const plain = new EventTarget();
console.log("plain typeof addEventListener:", typeof plain.addEventListener);
console.log("plain typeof removeEventListener:", typeof plain.removeEventListener);
console.log("plain typeof dispatchEvent:", typeof plain.dispatchEvent);
plain.addEventListener("ping", () => console.log("plain listener fired"));
console.log("plain dispatch returned:", plain.dispatchEvent(new Event("ping")));

// ── one-level subclass ──
class Bus extends EventTarget {}
const bus = new Bus();
console.log("sub typeof addEventListener:", typeof bus.addEventListener);
console.log("sub typeof dispatchEvent:", typeof bus.dispatchEvent);
bus.addEventListener("go", (e: Event) => {
  console.log("sub listener fired:", e.type);
});
console.log("sub dispatch returned:", bus.dispatchEvent(new Event("go")));

// ── two-level subclass: B extends A extends EventTarget ──
class A extends EventTarget {}
class B extends A {}
const deep = new B();
console.log("two-level typeof dispatchEvent:", typeof deep.dispatchEvent);
deep.addEventListener("deep", () => console.log("two-level listener fired"));
deep.dispatchEvent(new Event("deep"));

// ── a subclass with its own constructor + state (the cac shape) ──
class Emitter extends EventTarget {
  count: number;
  constructor(start: number) {
    super();
    this.count = start;
  }
  fire(name: string): void {
    // `this.dispatchEvent(...)` — exactly what cac's matched-command path does.
    this.dispatchEvent(new CustomEvent(name, { detail: { count: this.count } }));
  }
}
const em = new Emitter(7);
em.addEventListener("tick", (e: Event) => {
  const detail = (e as CustomEvent).detail as { count: number };
  console.log("ctor-subclass listener fired, detail.count:", detail.count);
});
em.fire("tick");
console.log("ctor-subclass field survived super():", em.count);

// ── the method value is callable when detached from the read ──
const add = bus.addEventListener.bind(bus);
add("bound", () => console.log("bound-read listener fired"));
bus.dispatchEvent(new Event("bound"));

// ── removeEventListener actually removes ──
const once = () => console.log("SHOULD NOT FIRE");
deep.addEventListener("gone", once);
deep.removeEventListener("gone", once);
deep.dispatchEvent(new Event("gone"));
console.log("removed listener did not fire");

// ── listener bags are per-instance, not shared across subclass instances ──
const busA = new Bus();
const busB = new Bus();
busA.addEventListener("only-a", () => console.log("busA listener fired"));
busB.dispatchEvent(new Event("only-a"));
console.log("busB has no busA listener");
busA.dispatchEvent(new Event("only-a"));

// ── an override on the subclass still wins over the inherited method ──
// The inherited surface is resolved as a LAST resort, after every own-property
// and class-vtable lookup, so a subclass that redefines one of these names
// keeps its own method. (This override deliberately does not delegate, so the
// registered listener must not fire.)
class Loud extends EventTarget {
  dispatchEvent(event: Event): boolean {
    console.log("override ran for:", event.type);
    return true;
  }
}
const loud = new Loud();
console.log("override-subclass inherits addEventListener:", typeof loud.addEventListener);
loud.addEventListener("x", () => console.log("SHOULD NOT FIRE"));
console.log("override returned:", loud.dispatchEvent(new Event("x")));

// ── options: { once: true } and event fields still work through a subclass ──
let onceCount = 0;
bus.addEventListener("tick-once", () => {
  onceCount++;
}, { once: true });
bus.dispatchEvent(new Event("tick-once"));
bus.dispatchEvent(new Event("tick-once"));
console.log("once listener call count:", onceCount);

// ── instanceof reaches the EventTarget base from both the base and a subclass ──
console.log("plain instanceof EventTarget:", plain instanceof EventTarget);
console.log("sub instanceof EventTarget:", bus instanceof EventTarget);
console.log("two-level instanceof EventTarget:", deep instanceof EventTarget);
console.log("plain object instanceof EventTarget:", {} instanceof EventTarget);

// ── EMPTY-SHAPE receivers: a subclass instance with no own properties at all ──
// The value-read fallback lives after the field-walk's `keys_array == null`
// early return, so these pin the invariant that makes that placement correct:
// a class instance ALWAYS carries a shape-cached keys array (a zero-LENGTH one
// when the class declares no fields), so it takes the shaped path, never the
// keyless one. Each of these has an empty own-key set at the moment the method
// is read.
class Empty extends EventTarget {
  constructor() {
    super(); // ctor that assigns nothing
  }
}
const empty = new Empty();
console.log("empty-ctor subclass:", typeof empty.addEventListener, typeof empty.dispatchEvent);

// constructed through a variable (dynamic `new`) rather than a static class ref
const BusRef = Bus;
const viaRef = new BusRef();
console.log("dynamic-new subclass:", typeof viaRef.addEventListener);

// constructed via Reflect.construct
const viaReflect = Reflect.construct(Bus, []) as Bus;
console.log("Reflect.construct subclass:", typeof viaReflect.addEventListener);

// a class *expression* bound to a const
const AnonBus = class extends EventTarget {};
const anon = new AnonBus();
anon.addEventListener("anon", () => console.log("class-expression listener fired"));
console.log("class-expression subclass:", typeof anon.dispatchEvent);
anon.dispatchEvent(new Event("anon"));

// a subclass declared inside a function body
function makeLocal(): void {
  class Local extends EventTarget {}
  const local = new Local();
  console.log("fn-local subclass:", typeof local.addEventListener);
  local.addEventListener("local", () => console.log("fn-local listener fired"));
  local.dispatchEvent(new Event("local"));
}
makeLocal();

// an instance whose only own property has been deleted (own-key set now empty)
class Deletable extends EventTarget {
  x?: number;
  constructor() {
    super();
    this.x = 1;
  }
}
const del = new Deletable();
delete del.x;
console.log("deleted-own-prop subclass:", typeof del.addEventListener, typeof del.dispatchEvent);
del.addEventListener("del", () => console.log("deleted-own-prop listener fired"));
del.dispatchEvent(new Event("del"));

// a frozen no-field subclass instance
const frozen = Object.freeze(new Bus());
console.log("frozen subclass:", typeof frozen.addEventListener, typeof frozen.dispatchEvent);

// ── Event / CustomEvent construction is untouched ──
const ev = new Event("basic");
console.log("event type:", ev.type, "cancelable:", ev.cancelable);
const ce = new CustomEvent("custom", { detail: { n: 42 } });
console.log("custom event type:", ce.type, "detail.n:", (ce.detail as { n: number }).n);
