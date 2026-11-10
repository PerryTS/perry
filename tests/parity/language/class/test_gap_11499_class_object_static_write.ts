// #11499: a property write whose receiver is the CLASS OBJECT must resolve
// against the STATIC accessor chain, never the instance vtable. Instance
// accessors live on `C.prototype`, which is not on the constructor's own
// prototype chain (`C -> Base -> Error -> Function.prototype`), so an instance
// `get x()` must not intercept `C.x = v`.
//
// `set_field_by_name_object_tail` consulted the instance vtable whenever the
// receiver carried `class_id != 0`, and a constructor object carries its own
// class's id — so the instance getter was found for a static write and refused
// it with "Cannot set property x of #<C> which has only a getter". cc 2.1.112
// died at startup on exactly this: an oauth error base declares
// `get errorCode()` for instances and each subclass assigns the static
// `Sub.errorCode = "..."`; every subcommand but `--version` was dead.
//
// The class identity is hidden behind a lazy initializer assigned to outer
// `var`s, and the write is done from a shared, IC-warmed call site. Both
// matter: a direct `const C = class … ; C.x = v` is resolved statically by
// codegen and never reaches the dynamic store path this bug lives on, so a
// reduction written that way passes even against the broken runtime.
//
// Output must be byte-identical to node.

function out(label: string, v: unknown): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}
function attempt(label: string, f: () => unknown): void {
  try {
    out(label, f());
  } catch (e: any) {
    out(label, "THREW " + (e && e.constructor ? e.constructor.name : "?"));
  }
}

let XX: any, VR8: any, Sy6: any;
const init = () => {
  XX = class XX extends Error {
    constructor(m?: string) {
      super(m);
      this.name = this.constructor.name;
    }
    get errorCode(): string {
      return (this.constructor as any).errorCode;
    }
  };
  VR8 = class VR8 extends XX {};
  Sy6 = class Sy6 extends XX {};
};
init();

// A shared put site, warmed with unrelated receiver shapes so the store is a
// polymorphic/packed miss by the time a class object reaches it.
function setCode(o: any, v: unknown): void {
  o.errorCode = v;
}
setCode({ a: 1 }, "x");
setCode({ b: 2, c: 3 }, "y");
setCode({ d: 4, e: 5, f: 6 }, "z");
setCode(new Map(), "w");

attempt("A.generic-site-on-class", () => {
  setCode(VR8, "invalid_request");
  return VR8.errorCode;
});
attempt("B.direct-on-var", () => {
  Sy6.errorCode = "invalid_client";
  return Sy6.errorCode;
});
attempt("C.computed-key", () => {
  const k = "errorCode";
  const T = class T extends XX {};
  T[k] = "ck";
  return T[k];
});
attempt("D.object-assign", () => {
  const T2 = class T2 extends XX {};
  Object.assign(T2, { errorCode: "oa" });
  return T2.errorCode;
});
attempt("E.reflect-set", () => {
  const T3 = class T3 extends XX {};
  Reflect.set(T3, "errorCode", "rs");
  return T3.errorCode;
});
// The instance accessor still reads through to the static it delegates to.
out("F.instance-getter", new VR8("boom").errorCode);

// A real `static set` must still fire, own and inherited through `extends`.
class HasStatic {
  static seen: unknown = null;
  static get tag(): unknown {
    return HasStatic.seen;
  }
  static set tag(v: unknown) {
    HasStatic.seen = "set:" + String(v);
  }
}
class SubStatic extends HasStatic {}
let P: any, Q: any;
const init2 = () => {
  P = HasStatic;
  Q = SubStatic;
};
init2();
attempt("G.own-static-setter", () => {
  setCode2(P, 1);
  return HasStatic.tag;
});
attempt("H.inherited-static-setter", () => {
  setCode2(Q, 2);
  return HasStatic.seen;
});
function setCode2(o: any, v: unknown): void {
  o.tag = v;
}

// Instance accessors keep intercepting INSTANCE writes.
class InstAcc {
  _v = 0;
  get v(): number {
    return this._v;
  }
  set v(n: number) {
    this._v = n * 10;
  }
}
const ia = new InstAcc();
ia.v = 4;
out("I.instance-setter-fired", ia.v);
