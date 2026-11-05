// Charter step 3, S3 G5/G6: a ClassBody accessor is ONE real accessor property
// of the class prototype, and every reflective operation — keys, entries,
// values, for-in, propertyIsEnumerable, hasOwnProperty, Object.hasOwn, `in`,
// Reflect.get with a receiver, defineProperty, delete — answers from that
// property alone. Each row changes the property (enumerable, redefined,
// converted, deleted) so an answer taken from the ClassBody declaration
// instead of the property would differ. Private `#x` accessors are never
// properties. Output must be byte-identical to node.

function out(label: string, v: unknown): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}

function forIn(o: object): string[] {
  const r: string[] = [];
  for (const k in o) r.push(k);
  return r;
}

function attrs(o: any, key: PropertyKey): string {
  const d = Object.getOwnPropertyDescriptor(o, key);
  if (!d) return "absent";
  const kind = "value" in d ? "data" : "accessor" + (d.get ? "g" : "-") + (d.set ? "s" : "-");
  return [kind, d.enumerable ? "e" : "ne", d.configurable ? "c" : "nc"].join("/");
}

// --- 1. enumerable accessor: every enumeration sees it once, in member order
class Enum {
  _a = 1;
  m() {
    return "m";
  }
  get a() {
    return this._a * 10;
  }
  get b() {
    return "b";
  }
  n() {
    return "n";
  }
}
Object.defineProperty(Enum.prototype, "a", { enumerable: true });
Object.defineProperty(Enum.prototype, "m", { enumerable: true });
out("1.keys", Object.keys(Enum.prototype));
out("1.entries", Object.entries(Enum.prototype).map(([k, v]) => k + "=" + typeof v));
out("1.values", Object.values(Enum.prototype).map((v) => typeof v));
out("1.for-in-proto", forIn(Enum.prototype));
out("1.for-in-inst", forIn(new Enum()));
out("1.pie", [
  Enum.prototype.propertyIsEnumerable("a"),
  Enum.prototype.propertyIsEnumerable("b"),
  Enum.prototype.propertyIsEnumerable("m"),
  Enum.prototype.propertyIsEnumerable("n"),
]);
out("1.attrs", [attrs(Enum.prototype, "a"), attrs(Enum.prototype, "b")]);

// --- 2. enumerable back off, then the getter reads the receiver
Object.defineProperty(Enum.prototype, "a", { enumerable: false });
out("2.keys", Object.keys(Enum.prototype));
out("2.pie", Enum.prototype.propertyIsEnumerable("a"));
out("2.for-in-inst", forIn(new Enum()));
{
  const e = new Enum();
  e._a = 4;
  out("2.read", [e.a, Reflect.get(Enum.prototype, "a", { _a: 7 })]);
}

// --- 3. presence after redefine, conversion and delete
class Pres {
  get p() {
    return "p";
  }
  set q(v: string) {}
  get r() {
    return "r";
  }
}
const pp: any = Pres.prototype;
const pi: any = new Pres();
out("3.before", [
  pp.hasOwnProperty("p"),
  Object.hasOwn(pp, "q"),
  "r" in pp,
  "p" in pi,
  Object.hasOwn(pi, "p"),
]);
Object.defineProperty(pp, "p", { value: "data-p", writable: true, enumerable: true, configurable: true });
delete pp.q;
Object.defineProperty(pp, "r", {
  get() {
    return "r2";
  },
  configurable: true,
});
out("3.after", [
  attrs(pp, "p"),
  attrs(pp, "q"),
  attrs(pp, "r"),
  pp.hasOwnProperty("q"),
  Object.hasOwn(pp, "q"),
  "q" in pp,
  "q" in pi,
  Object.getOwnPropertyNames(pp),
]);
out("3.reads", [pi.p, pi.q, pi.r]);
out("3.keys", [Object.keys(pp), forIn(pi)]);
out("3.entries", Object.entries(pp));

// --- 4. Reflect.get with a receiver walks the real chain
class RBase {
  tag = "base-inst";
  get who() {
    return "who:" + (this as any).tag;
  }
}
class RMid extends RBase {}
class RLeaf extends RMid {
  get leaf() {
    return "leaf:" + (this as any).tag;
  }
}
const recv = { tag: "receiver" };
out("4.inherited", [
  Reflect.get(RLeaf.prototype, "who", recv),
  Reflect.get(new RLeaf(), "who", recv),
  Reflect.get(RLeaf.prototype, "leaf", recv),
]);
// a data property on an intermediate prototype shadows the accessor
Object.defineProperty(RMid.prototype, "who", { value: "mid-data", configurable: true });
out("4.shadowed", [Reflect.get(RLeaf.prototype, "who", recv), new RLeaf().who]);
delete (RMid.prototype as any).who;
out("4.unshadowed", Reflect.get(new RLeaf(), "who", recv));
// the accessor replaced on the base prototype
Object.defineProperty(RBase.prototype, "who", {
  get() {
    return "new-who:" + this.tag;
  },
  configurable: true,
});
out("4.replaced", [Reflect.get(new RLeaf(), "who", recv), new RLeaf().who]);
delete (RBase.prototype as any).who;
out("4.deleted", [Reflect.get(new RLeaf(), "who", recv), "who" in new RLeaf()]);

// --- 5. an accessor added to a class prototype after the fact
class Late {
  own() {
    return 1;
  }
}
Object.defineProperty(Late.prototype, "late", {
  get() {
    return "late:" + typeof this;
  },
  enumerable: true,
  configurable: true,
});
out("5.late", [
  (new Late() as any).late,
  Object.keys(Late.prototype),
  forIn(new Late()),
  "late" in new Late(),
  Late.prototype.hasOwnProperty("late"),
  Late.prototype.propertyIsEnumerable("late"),
  attrs(Late.prototype, "late"),
]);

// --- 6. static accessors keep their attributes on the constructor
class Stat {
  static get s() {
    return "s";
  }
  static set t(v: number) {}
  static field = 1;
}
out("6.before", [Object.keys(Stat), attrs(Stat, "s"), Stat.propertyIsEnumerable("s")]);
Object.defineProperty(Stat, "s", { enumerable: true });
Object.defineProperty(Stat, "t", { configurable: false });
out("6.after", [
  Object.keys(Stat),
  attrs(Stat, "s"),
  attrs(Stat, "t"),
  Stat.propertyIsEnumerable("s"),
  forIn(Stat),
  (Stat as any).s,
]);

// --- 7. private accessors are not properties
class Priv {
  #v = 1;
  get #x() {
    return this.#v * 100;
  }
  set #x(v: number) {
    this.#v = v;
  }
  get #ro() {
    return "ro";
  }
  set #wo(v: string) {
    this.#v = v.length;
  }
  bump() {
    this.#x = this.#x + 1;
    return this.#x;
  }
  readRo() {
    return this.#ro;
  }
  writeRo() {
    try {
      // @ts-ignore
      this.#ro = "no";
      return "wrote";
    } catch (e) {
      return (e as Error).constructor.name;
    }
  }
  readWo() {
    try {
      // @ts-ignore
      return String(this.#wo);
    } catch (e) {
      return (e as Error).constructor.name;
    }
  }
  writeWo(s: string) {
    this.#wo = s;
    return this.#x;
  }
  static has(o: object) {
    return #x in o;
  }
  static readX(o: any) {
    try {
      return o.#x;
    } catch (e) {
      return (e as Error).constructor.name;
    }
  }
}
{
  const p = new Priv();
  out("7.ops", [p.bump(), p.readRo(), p.writeRo(), p.readWo(), p.writeWo("abcd")]);
  out("7.brand", [Priv.has(p), Priv.has({}), Priv.readX({ "#x": 5 })]);
  out("7.names", [
    Object.getOwnPropertyNames(Priv.prototype),
    Reflect.ownKeys(p),
    "#x" in p,
    (p as any)["#x"],
    attrs(Priv.prototype, "#x"),
  ]);
  // a public string property spelled "#x" is unrelated to the private name
  (p as any)["#x"] = "public";
  out("7.public-spelling", [(p as any)["#x"], p.bump(), Object.keys(p)]);
  class PrivSub extends Priv {}
  out("7.sub", [Priv.has(new PrivSub()), new PrivSub().bump()]);
}

// --- 8. array-like `length` resolved on the class prototype
class LenRO {
  get length() {
    return 1;
  }
}
{
  // a getter-only `length` on the class prototype rejects Set(O, "length")
  const r: any = new LenRO();
  r[0] = "only";
  let caught = "none";
  try {
    Array.prototype.splice.call(r, 0, 1);
  } catch (e) {
    caught = (e as Error).constructor.name;
  }
  out("8.getter-only", caught);
  // an own data `length` shadows the prototype accessor
  const o: any = new LenRO();
  Object.defineProperty(o, "length", { value: 0, writable: true, configurable: true });
  out("8.own", [o.length, Object.getOwnPropertyNames(o)]);
}

// --- 9. an own data property on an instance shadows the class getter
class Shadow {
  get v() {
    return "getter";
  }
}
{
  const s: any = new Shadow();
  Object.defineProperty(s, "v", { value: "own", enumerable: true });
  const anyRead = (o: any, k: string) => o[k];
  out("9.shadow", [s.v, anyRead(s, "v"), Object.keys(s), JSON.stringify(s)]);
}
