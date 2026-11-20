// #321 (effect Context/Layer/Scope): a STRING-KEYED method INHERITED via
// `Object.create(proto)` must run with `this` bound to the RECEIVER, not the
// prototype it is defined on.
//
//   const Proto = { getState(this: any) { return this.state } }
//   const o = Object.create(Proto); o.state = { tag: "X" }
//   o.getState()   // Node -> { tag: 'X' };  pre-fix Perry -> undefined
//
// Root cause: object-literal methods are lowered with `captures_this:true` and
// have their reserved capture slot patched to the LITERAL object (the
// prototype) at construction time. Resolving `o.method()` walked the prototype
// chain and returned the prototype's closure verbatim — whose baked `this`
// slot points at the prototype, so the body read `this === proto` and
// `this.<field>` was undefined (the receiver's own field lived on `o`).
// Setting `IMPLICIT_THIS = o` couldn't override the baked-in slot.
//
// Fix: when a string-keyed method is resolved off the prototype chain (i.e. it
// is INHERITED, not an own field), rebind the closure's `this` slot to the
// receiver before invoking — the same treatment the symbol-keyed path got in
// #1969. Own methods (whose baked slot already IS the receiver) and class
// methods are untouched.
//
// Compared byte-for-byte against `node --experimental-strip-types`.

// (1) inherited method reading `this.state` (whole object) and a nested field.
const Proto: any = {
  getState(this: any) {
    return this.state;
  },
  tagOf(this: any) {
    return this.state && this.state.tag;
  },
};
const o = Object.create(Proto);
o.state = { tag: "X" };
console.log("external read:", o.state.tag); // X
console.log("method this.state:", o.getState()); // { tag: 'X' }
console.log("method tagOf:", o.tagOf()); // X

// (2) own string method — `this` is the object itself (must NOT regress).
const own: any = {
  state: { tag: "OWN" },
  getState(this: any) {
    return this.state.tag;
  },
};
console.log("own:", own.getState()); // OWN

// (3) class method — `this` is the instance (must NOT regress).
class C {
  state = { tag: "CLASS" };
  getState() {
    return this.state.tag;
  }
}
console.log("class:", new C().getState()); // CLASS

// (4) two-level Object.create chain — `this` is the leaf receiver, with a
//     mid-level field correctly shadowed by the leaf.
const Base: any = {
  describe(this: any) {
    return this.label + ":" + this.n;
  },
};
const Mid = Object.create(Base);
Mid.n = 0;
const leaf = Object.create(Mid);
leaf.label = "LEAF";
leaf.n = 7;
console.log("proto2:", leaf.describe()); // LEAF:7

// (5) inherited method that calls ANOTHER inherited method through `this`.
const P2: any = {
  base() {
    return "B";
  },
  wrap(this: any) {
    return "[" + this.base() + this.suffix + "]";
  },
};
const child = Object.create(P2);
child.suffix = "S";
console.log("nested-this:", child.wrap()); // [BS]
