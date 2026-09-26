// Charter step 3, accessor stage: a ClassBody `get`/`set` is a real accessor
// property of the class prototype. Reflection must see ONE property with a
// stable identity, in ClassBody order among the methods, with attributes that
// a generic define can change and a delete can remove. Output must be
// byte-identical to node.

function out(label: string, v: unknown): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}

function summary(o: any, key: string): string {
  const d = Object.getOwnPropertyDescriptor(o, key);
  if (!d) return "absent";
  const kind = "value" in d ? "data" : "accessor";
  const halves = kind === "accessor" ? (d.get ? "g" : "-") + (d.set ? "s" : "-") : "";
  return [kind + halves, d.enumerable ? "e" : "ne", d.configurable ? "c" : "nc"].join("/");
}

class Point {
  _x = 1;
  _y = 2;
  first() {
    return "first";
  }
  get x() {
    return this._x;
  }
  set x(v: number) {
    this._x = v;
  }
  middle() {
    return "middle";
  }
  get y() {
    return this._y;
  }
  last() {
    return "last";
  }
}

// --- 1. own property order interleaves accessors with methods
out("1.names", Object.getOwnPropertyNames(Point.prototype));
out("1.keys", Object.keys(Point.prototype));

// --- 2. one property, stable identity of each half
{
  const a = Object.getOwnPropertyDescriptor(Point.prototype, "x")!;
  const b = Object.getOwnPropertyDescriptor(Point.prototype, "x")!;
  out("2.same-get", a.get === b.get);
  out("2.same-set", a.set === b.set);
  out("2.names", [a.get!.name, a.set!.name, a.get!.length, a.set!.length]);
  out("2.attrs", [summary(Point.prototype, "x"), summary(Point.prototype, "y")]);
  out("2.has", [
    Object.prototype.hasOwnProperty.call(Point.prototype, "x"),
    "y" in Point.prototype,
    Object.prototype.hasOwnProperty.call(new Point(), "x"),
  ]);
}

// --- 3. calling a reflected half with an explicit receiver
{
  const d = Object.getOwnPropertyDescriptor(Point.prototype, "x")!;
  const p = new Point();
  d.set!.call(p, 41);
  out("3.call", [d.get!.call(p), p._x]);
}

// --- 4. a generic define changes only the attributes
class Flags {
  get on() {
    return true;
  }
}
Object.defineProperty(Flags.prototype, "on", { enumerable: true });
out("4.attrs", summary(Flags.prototype, "on"));
out("4.keys", Object.keys(Flags.prototype));
out("4.read", new Flags().on);
{
  const e: string[] = [];
  for (const k in new Flags()) e.push(k);
  out("4.for-in", e);
}

// --- 5. delete removes the property from reflection
class Gone {
  get g() {
    return 1;
  }
  keep() {
    return 2;
  }
}
out("5.before", [summary(Gone.prototype, "g"), Object.getOwnPropertyNames(Gone.prototype)]);
out("5.delete", delete (Gone.prototype as any).g);
out("5.after", [summary(Gone.prototype, "g"), Object.getOwnPropertyNames(Gone.prototype)]);

// --- 6. a non-configurable accessor refuses delete
class Pinned {
  get p() {
    return 3;
  }
}
Object.defineProperty(Pinned.prototype, "p", { configurable: false });
out("6.attrs", summary(Pinned.prototype, "p"));
out("6.delete", Reflect.deleteProperty(Pinned.prototype, "p"));
out("6.still", summary(Pinned.prototype, "p"));

// --- 7. inheritance: a subclass prototype does not own the parent's accessor
class Base {
  get kind() {
    return "base";
  }
}
class Derived extends Base {
  get extra() {
    return "derived";
  }
}
out("7.own", [
  Object.getOwnPropertyNames(Derived.prototype),
  summary(Derived.prototype, "kind"),
  summary(Base.prototype, "kind"),
]);
out("7.read", [new Derived().kind, new Derived().extra]);

// --- 8. a computed accessor key
const dyn = "computed" + String(1);
class Computed {
  get [dyn]() {
    return "c1";
  }
  static get s() {
    return "static";
  }
}
out("8.names", [Object.getOwnPropertyNames(Computed.prototype), summary(Computed.prototype, dyn)]);
out("8.static", [summary(Computed, "s"), (Computed as any).s]);
out("8.read", (new Computed() as any)[dyn]);

// --- 9. reads resolve the property on the prototype, not a side record
class Live {
  _v = 5;
  get v() {
    return this._v;
  }
  set only(x: number) {
    this._v = x;
  }
}
{
  const a: any = new Live();
  out("9.read", [a.v, a.only]);
  Object.defineProperty(Live.prototype, "v", {
    get() {
      return "replaced:" + this._v;
    },
    configurable: true,
  });
  out("9.replaced", a.v);
  delete (Live.prototype as any).v;
  out("9.deleted", [a.v, "v" in a]);
  class Sub extends Live {}
  const s: any = new Sub();
  s.only = 9;
  out("9.sub", [s._v, s.only]);
}
