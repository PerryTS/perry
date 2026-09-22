// #10480: an attributes-only descriptor on an existing CLASS accessor must
// keep its getter and setter and change only the attributes.
//
// `Object.defineProperties(C.prototype, { p: { enumerable: true } })` is how
// every WebIDL-generated class (whatwg-url, node-fetch, undici-style
// polyfills) publishes its prototype accessors. Perry's define path only knew
// the address-keyed descriptor tables, which never hold a ClassBody accessor,
// so it filed the key as a brand-new property with a `writable: false` data
// slot on the prototype: the setter stopped running (assignment threw
// "Cannot assign to read only property" in strict code, and was dropped in
// sloppy code), and the requested `enumerable` / `configurable` never showed
// up in the descriptor or in `Object.keys` / `for...in`.
//
// Covered here: get/set pairs, getter-only, setter-only, static accessors,
// `defineProperty` vs `defineProperties`, subclass instances, the
// non-configurable rejections, and the object-literal / `defineProperty` /
// function-prototype accessors that always worked (controls).

function describe(object: any, key: string): string {
  const descriptor = Object.getOwnPropertyDescriptor(object, key);
  if (!descriptor) return "absent";
  const kind =
    "get" in descriptor
      ? `get=${typeof descriptor.get} set=${typeof descriptor.set}`
      : `value=${typeof descriptor.value} writable=${descriptor.writable}`;
  return `${kind} enumerable=${descriptor.enumerable} configurable=${descriptor.configurable}`;
}

function outcome(fn: () => string): string {
  try {
    return fn();
  } catch (error: any) {
    return `${error.constructor.name}`;
  }
}

function forIn(object: any): string {
  const keys: string[] = [];
  for (const key in object) keys.push(key);
  return keys.join(",");
}

// ── the node-fetch / whatwg-url shape ───────────────────────────────────────
class URLLike {
  _p = "";
  get pathname() {
    return this._p;
  }
  set pathname(value: string) {
    this._p = "set:" + value;
  }
}
Object.defineProperties(URLLike.prototype, { pathname: { enumerable: true } });
console.log("url-desc", describe(URLLike.prototype, "pathname"));
const url = new URLLike();
console.log(
  "url-assign",
  outcome(() => {
    url.pathname = "/x";
    return url.pathname;
  }),
);
console.log("url-keys", Object.keys(URLLike.prototype).join(","));
console.log("url-for-in", forIn(url));
console.log(
  "url-enumerable",
  URLLike.prototype.propertyIsEnumerable("pathname"),
  Object.prototype.propertyIsEnumerable.call(URLLike.prototype, "pathname"),
);

// A dynamic receiver takes the runtime dispatch path rather than a compiled
// direct call to the declared setter.
const dynamicUrl: any = new URLLike();
const dynamicKey = "pathname";
dynamicUrl[dynamicKey] = "/dyn";
console.log("url-dynamic", dynamicUrl[dynamicKey]);

// ── defineProperty, every generic descriptor shape ──────────────────────────
class Single {
  _p = "";
  get p() {
    return this._p;
  }
  set p(value: string) {
    this._p = "set:" + value;
  }
}
Object.defineProperty(Single.prototype, "p", { configurable: true });
const single = new Single();
console.log(
  "single-configurable",
  outcome(() => {
    single.p = "y";
    return single.p;
  }),
  describe(Single.prototype, "p"),
);
Object.defineProperty(Single.prototype, "p", {});
console.log("single-empty", describe(Single.prototype, "p"));
Object.defineProperty(Single.prototype, "p", { enumerable: true });
console.log("single-enumerable", describe(Single.prototype, "p"), Object.keys(Single.prototype).join(","));
Object.defineProperty(Single.prototype, "p", { enumerable: false });
console.log("single-non-enumerable", describe(Single.prototype, "p"), Object.keys(Single.prototype).join(","));
console.log(
  "single-still-set",
  outcome(() => {
    single.p = "z";
    return single.p;
  }),
);

// ── getter-only and setter-only halves ──────────────────────────────────────
class Halves {
  _v = 0;
  get readOnly() {
    return "read:" + this._v;
  }
  set writeOnly(value: number) {
    this._v = value + 1;
  }
}
Object.defineProperties(Halves.prototype, {
  readOnly: { enumerable: true },
  writeOnly: { enumerable: true },
});
console.log("halves-read", describe(Halves.prototype, "readOnly"));
console.log("halves-write", describe(Halves.prototype, "writeOnly"));
const halves: any = new Halves();
console.log("halves-get", halves.readOnly);
halves.writeOnly = 41;
console.log("halves-set", halves._v, halves.readOnly);
console.log("halves-keys", Object.keys(Halves.prototype).join(","));
console.log("halves-for-in", forIn(halves));
console.log("halves-enumerable", Halves.prototype.propertyIsEnumerable("readOnly"));

// ── inheritance: the accessor is redefined on the BASE prototype ────────────
class Base {
  _b = "";
  get tag() {
    return this._b;
  }
  set tag(value: string) {
    this._b = "base:" + value;
  }
}
class Derived extends Base {}
Object.defineProperties(Base.prototype, { tag: { enumerable: true } });
const derived = new Derived();
console.log(
  "derived-assign",
  outcome(() => {
    derived.tag = "v";
    return derived.tag;
  }),
);
console.log("derived-for-in", forIn(derived));
console.log("derived-own", Object.keys(Derived.prototype).join(","), Object.keys(Base.prototype).join(","));

// ── ClassBody order is preserved when several accessors go enumerable ───────
class Ordered {
  get b() {
    return "b";
  }
  m() {
    return "m";
  }
  get a() {
    return "a";
  }
  get c() {
    return "c";
  }
}
Object.defineProperties(Ordered.prototype, { c: { enumerable: true }, b: { enumerable: true } });
console.log("ordered-keys", Object.keys(Ordered.prototype).join(","));
console.log("ordered-names", Object.getOwnPropertyNames(Ordered.prototype).join(","));
console.log("ordered-entries", JSON.stringify(Object.entries(Ordered.prototype)));

// ── static accessors ────────────────────────────────────────────────────────
class Statics {
  static _v = 1;
  static get sv() {
    return Statics._v;
  }
  static set sv(value: number) {
    Statics._v = value * 10;
  }
}
Object.defineProperty(Statics, "sv", { enumerable: true });
console.log("static-desc", describe(Statics, "sv"));
console.log(
  "static-enumerable",
  Statics.propertyIsEnumerable("sv"),
  Object.prototype.propertyIsEnumerable.call(Statics, "sv"),
);
Statics.sv = 2;
console.log("static-read", Statics.sv, Object.keys(Statics).join(","), forIn(Statics));

// ── non-configurable rejections ─────────────────────────────────────────────
class Locked {
  get q() {
    return 1;
  }
  set q(_value: number) {}
}
Object.defineProperty(Locked.prototype, "q", { configurable: false });
console.log("locked-desc", describe(Locked.prototype, "q"));
console.log(
  "locked-configurable",
  outcome(() => {
    Object.defineProperty(Locked.prototype, "q", { configurable: true });
    return "ok";
  }),
);
console.log(
  "locked-enumerable",
  outcome(() => {
    Object.defineProperty(Locked.prototype, "q", { enumerable: true });
    return "ok";
  }),
);
console.log(
  "locked-getter",
  outcome(() => {
    Object.defineProperty(Locked.prototype, "q", {
      get() {
        return 2;
      },
    });
    return "ok";
  }),
);
console.log(
  "locked-data",
  outcome(() => {
    Object.defineProperty(Locked.prototype, "q", { value: 3 });
    return "ok";
  }),
);
console.log(
  "locked-same",
  outcome(() => {
    Object.defineProperty(Locked.prototype, "q", { configurable: false, enumerable: false });
    return "ok";
  }),
);
let deleted: unknown = "unset";
try {
  deleted = delete (Locked.prototype as any).q;
} catch (error: any) {
  deleted = error instanceof TypeError;
}
console.log("locked-delete", deleted, describe(Locked.prototype, "q"));
console.log("locked-read", new Locked().q);

// ── controls: shapes that always worked ─────────────────────────────────────
const literal: any = {
  _v: "",
  get p() {
    return this._v;
  },
  set p(value: string) {
    this._v = "set:" + value;
  },
};
Object.defineProperties(literal, { p: { enumerable: true } });
literal.p = "lit";
console.log("literal", literal.p, describe(literal, "p"), Object.keys(literal).join(","));

const made: any = {};
Object.defineProperty(made, "p", {
  get() {
    return this._v;
  },
  set(value: string) {
    this._v = "set:" + value;
  },
  configurable: true,
});
Object.defineProperties(made, { p: { enumerable: true } });
made.p = "def";
console.log("defined", made.p, describe(made, "p"));

function Legacy(this: any) {}
Object.defineProperty(Legacy.prototype, "p", {
  get() {
    return this._v;
  },
  set(value: string) {
    this._v = "set:" + value;
  },
  configurable: true,
});
Object.defineProperties(Legacy.prototype, { p: { enumerable: true } });
const legacy: any = new (Legacy as any)();
legacy.p = "fn";
console.log("function-prototype", legacy.p, describe(Legacy.prototype, "p"), forIn(legacy));

// A class METHOD is a data property; a generic descriptor must keep its value.
class WithMethod {
  m() {
    return "m";
  }
}
Object.defineProperty(WithMethod.prototype, "m", { enumerable: true });
console.log("method", new WithMethod().m(), describe(WithMethod.prototype, "m"));
