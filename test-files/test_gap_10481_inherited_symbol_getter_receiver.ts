// #10481: an INHERITED Symbol-keyed accessor must run with the ORIGINAL
// receiver as `this` ([[Get]](P, Receiver) / [[Set]](P, V, Receiver)), for
// every read form, at any prototype depth, whatever built the prototype.
// fastify 5's `lib/reply.js` `[kRouteContext]` getter crashed every request
// with `this === undefined`.

function show(label: string, f: () => unknown): void {
  try {
    const v = f();
    console.log(label, typeof v === "symbol" ? String(v) : JSON.stringify(v));
  } catch (e: any) {
    console.log(label, "THREW", e instanceof TypeError ? "TypeError" : String(e), e.message ?? "");
  }
}

// ---------------------------------------------------------------------------
// 1. fastify lib/reply.js shape: function constructor + defineProperties.
// ---------------------------------------------------------------------------
const kRouteContext = Symbol("kRouteContext");
function Reply(this: any, request: any) {
  this.request = request;
}
Object.defineProperties(Reply.prototype, {
  [kRouteContext]: {
    get() {
      return this.request[kRouteContext];
    },
  },
  routeOptions: {
    get() {
      return this.request[kRouteContext];
    },
  },
});
const reply: any = new (Reply as any)({ [kRouteContext]: "ctx" });
show("1a symbol-keyed prototype getter (fn ctor):", () => reply[kRouteContext]);
show("1b string-keyed prototype getter (control):", () => reply.routeOptions);

const kAsAny: any = kRouteContext;
show("1c symbol getter through an any-typed key var:", () => reply[kAsAny]);

show("1d optional chaining on the same getter:", () => reply?.[kRouteContext]);

show("1e destructured symbol-keyed read:", () => {
  const { [kRouteContext]: v } = reply;
  return v;
});

console.log("1f kRouteContext in reply:", kRouteContext in reply);

// ---------------------------------------------------------------------------
// 2. object-literal `get [sym]()` reached through Object.create, one and two
//    levels deep.
// ---------------------------------------------------------------------------
const k = Symbol("k");
const proto: any = {
  get [k]() {
    return this === undefined ? "this===undefined" : this.v;
  },
};
const child = Object.create(proto);
child.v = 7;
const grandchild = Object.create(child);
grandchild.v = 9;

show("2a inherited literal getter, one level (Object.create):", () => child[k]);
show("2b inherited literal getter, two levels (Object.create):", () => grandchild[k]);

// ---------------------------------------------------------------------------
// 3. own symbol getter (control) — must be unaffected.
// ---------------------------------------------------------------------------
const own: any = { v: 8 };
Object.defineProperty(own, k, {
  get() {
    return this.v;
  },
});
show("3a own symbol getter (control):", () => own[k]);

// ---------------------------------------------------------------------------
// 4. Reflect.get — with and without an explicit receiver.
// ---------------------------------------------------------------------------
show("4a Reflect.get(child, k) (default receiver = child):", () => Reflect.get(child, k));
const other: any = { v: 100 };
show("4b Reflect.get(child, k, other) (explicit receiver):", () => Reflect.get(child, k, other));

// ---------------------------------------------------------------------------
// 5. a nearer own data property shadows an inherited accessor.
// ---------------------------------------------------------------------------
const shadowed: any = Object.create(proto);
shadowed.v = 1;
// `proto`'s `[k]` is getter-only, so a plain `shadowed[k] = ...` would walk up
// to it and throw (no setter) in strict mode; Object.defineProperty creates
// the OWN data property directly, without going through [[Set]].
Object.defineProperty(shadowed, k, { value: "own-data", writable: true, enumerable: true, configurable: true });
show("5a own data property shadows inherited accessor (read):", () => shadowed[k]);

// ---------------------------------------------------------------------------
// 6. write forms — an inherited setter must run with the receiver (each
//    instance keeps its own state), and Reflect.set must reach it too.
// ---------------------------------------------------------------------------
const wproto: any = {
  _store: new Map<any, unknown>(),
  get [k]() {
    return this._store.get(this);
  },
  set [k](v: unknown) {
    this._store.set(this, v);
  },
};
const w1: any = Object.create(wproto);
const w2: any = Object.create(wproto);
w1[k] = "w1-value";
w2[k] = "w2-value";
show("6a inherited setter keeps per-receiver state (w1):", () => w1[k]);
show("6b inherited setter keeps per-receiver state (w2):", () => w2[k]);
console.log(
  "6c write did not create a shadowing own property:",
  Object.prototype.hasOwnProperty.call(w1, k) === false &&
    Object.prototype.hasOwnProperty.call(w2, k) === false,
);

Reflect.set(w1, k, "w1-via-reflect");
show("6d Reflect.set through the inherited setter:", () => w1[k]);

// ---------------------------------------------------------------------------
// 7. declared class prototype accessor, inherited by a subclass instance
//    (a field the subclass does NOT itself declare, so the subclass's own
//    field initializer can't shadow anything — isolates receiver identity
//    from unrelated field-initialization-order concerns).
// ---------------------------------------------------------------------------
const kTag = Symbol("kTag");
class Base {}
Object.defineProperty(Base.prototype, kTag, {
  get() {
    return (this as any).ownVal;
  },
});
class Sub extends Base {
  ownVal = "sub-own";
}
const sub = new Sub();
show("7a inherited getter on a declared class prototype:", () => (sub as any)[kTag]);

// ---------------------------------------------------------------------------
// 8. Symbol.toStringTag through Object.prototype.toString, inherited.
// ---------------------------------------------------------------------------
function Widget(this: any) {}
Object.defineProperty(Widget.prototype, Symbol.toStringTag, {
  get() {
    return "MyWidget";
  },
});
const widget = new (Widget as any)();
console.log("8a inherited Symbol.toStringTag getter:", Object.prototype.toString.call(widget));

// ---------------------------------------------------------------------------
// 9. two-level inheritance via Object.setPrototypeOf on function prototypes.
// ---------------------------------------------------------------------------
function R(this: any, request: any) {
  this.request = request;
}
Object.defineProperty(R.prototype, kRouteContext, {
  get() {
    return this.request[kRouteContext];
  },
});
function Two(this: any, request: any) {
  this.request = request;
}
Object.setPrototypeOf(Two.prototype, R.prototype);
const two: any = new (Two as any)({ [kRouteContext]: "two-ctx" });
show("9a two-level inherited getter via setPrototypeOf:", () => two[kRouteContext]);
