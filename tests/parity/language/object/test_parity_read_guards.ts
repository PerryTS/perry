// Differential probe for the generic property-read hit path after three
// guards left it: the GC-kind load (#10828), the descriptor-flag test
// (#10824) and the TAG_HOLE compare (#10826). Every read below goes through
// ONE shared site that has been primed on a plain object, so a wrong-value
// bug shows up as a raw slot served for a receiver the ShapeId compare
// should have refused. Run by both Perry and node; outputs are diffed.
//
// Each section names the guard that used to catch its receivers.

function read(o: any): any {
  return o.a;
}
function prime(o: any): void {
  for (let i = 0; i < 64; i++) read(o);
}
function show(label: string, v: any): void {
  console.log(label, typeof v, String(v));
}

// ---- descriptor flag (#10824 / #6080) ---------------------------------------
{
  const o: any = { a: 1, b: 2 };
  prime(o);
  Object.defineProperty(o, "a", { get() { return 42; }, configurable: true });
  show("accessor-after-prime", read(o));
  Object.defineProperty(o, "a", { value: 7, writable: false, configurable: true });
  show("data-descriptor-after-accessor", read(o));
  delete o.a;
  show("deleted-descriptor-key", read(o));
  // Re-adding the key after this delete is silently dropped on pristine main
  // (v0.5.1618) as well as on this branch: #10840, a pre-existing defect in
  // the descriptor/delete path (a deleted NON-WRITABLE key cannot be
  // re-added). Deliberately not exercised here so this file stays
  // byte-identical to node.
}
{
  const o: any = { a: 3, b: 4 };
  prime(o);
  Object.freeze(o);
  show("frozen-after-prime", read(o));
  try { o.a = 100; } catch (e) { /* strict-mode TypeError is fine either way */ }
  show("frozen-write-ignored", read(o));
}
{
  const o: any = { a: 5 };
  prime(o);
  Object.seal(o);
  o.a = 6;
  show("sealed-write-lands", read(o));
}
{
  // Getter installed on the PROTOTYPE after the site primed on an own slot:
  // the own slot still wins, and once deleted the getter must be observed.
  const proto: any = {};
  const o: any = Object.create(proto);
  o.a = "own";
  prime(o);
  Object.defineProperty(proto, "a", { get() { return "proto-getter"; } });
  show("own-shadows-proto-getter", read(o));
  delete o.a;
  show("proto-getter-after-delete", read(o));
}

// ---- TAG_HOLE compare (#10826 / #9064) ---------------------------------------
{
  const o: any = { a: 1, b: 2, c: 3 };
  prime(o);
  delete o.a;
  show("deleted-after-prime", read(o));
  show("in-after-delete", "a" in o);
  o.a = 11;
  show("re-added-after-delete", read(o));
  delete o.b;
  show("other-key-deleted", read(o));
  delete o.a;
  delete o.c;
  show("all-deleted", read(o));
  console.log("keys-after-churn", JSON.stringify(Object.keys(o)));
}
{
  const proto: any = { a: "from-proto" };
  const o: any = Object.create(proto);
  o.a = "own";
  prime(o);
  delete o.a;
  show("delete-falls-to-proto", read(o));
}
{
  // Delete inside a rotation the ways can hold (5 shapes), then re-read all.
  const objs: any[] = [
    { a: 0, x: 0 }, { a: 1, y: 1 }, { a: 2, z: 2 }, { a: 3, w: 3 }, { a: 4, v: 4 },
  ];
  for (let i = 0; i < 200; i++) read(objs[i % 5]);
  delete objs[2].a;
  const got: any[] = [];
  for (let i = 0; i < 5; i++) got.push(read(objs[i]));
  console.log("polymorphic-delete", JSON.stringify(got));
}
{
  // A key past the inline region (spill-located) survives a delete of an
  // inline key and disappears on its own delete.
  const o: any = {};
  for (let i = 0; i < 20; i++) o["k" + i] = i;
  o.a = "spill";
  prime(o);
  delete o.k3;
  show("spill-after-inline-delete", read(o));
  delete o.a;
  show("spill-deleted", read(o));
}
{
  // Class instance: field deleted in the constructor, then re-added.
  class C { a = 7; constructor() { delete (this as any).a; } }
  const c: any = new C();
  prime(c);
  show("class-field-deleted-in-ctor", read(c));
  c.a = 8;
  show("class-field-re-added", read(c));
}

// ---- GC-kind load (#10828 / #72) -----------------------------------------------
{
  const plain: any = { a: "plain" };
  prime(plain);
  const receivers: Array<[string, any]> = [
    ["date-minus-one", new Date(-1)],
    ["date-now", new Date(0)],
    ["array", [1, 2, 3]],
    ["map", new Map([[1, 2]])],
    ["set", new Set([1])],
    ["uint8array", new Uint8Array(4)],
    ["int8array", new Int8Array(4)],
    ["arraybuffer", new ArrayBuffer(8)],
    ["regexp", /re/g],
    ["error", new Error("x")],
    ["promise", Promise.resolve(1)],
    ["function", function () {}],
    ["arrow", () => 1],
    ["class", class K {}],
    ["symbol", Symbol("s")],
    ["heap-string", "a heap string longer than sso"],
    ["sso-string", "sso"],
    ["number", 3.5],
    ["bigint", 12345678901234567890n],
    ["boolean", true],
  ];
  for (const [label, r] of receivers) {
    show("kind:" + label, read(r));
  }
  // Expandos on the exotic kinds are still served (through the slow path).
  const d: any = new Date(-1);
  d.a = "date-expando";
  show("date-expando", read(d));
  const arr: any = [1, 2];
  arr.a = "array-expando";
  show("array-expando", read(arr));
  const fn: any = function () {};
  fn.a = "fn-expando";
  show("fn-expando", read(fn));
  const re: any = /x/;
  re.a = "re-expando";
  show("re-expando", read(re));
  show("plain-still-hits", read(plain));
}
{
  // `.size` keeps its native Map/Set arm; a plain object with a `size` key
  // takes the shape compare.
  function size(o: any): any { return o.size; }
  const s = new Set([1, 2, 3]);
  const m = new Map([["k", 1]]);
  const o = { size: "own-size" };
  for (let i = 0; i < 64; i++) { size(s); size(m); size(o); }
  show("set-size", size(s));
  show("map-size", size(m));
  show("object-size", size(o));
  show("date-size", size(new Date(0)));
}
{
  // `.length` on a dynamically typed receiver: strings, arrays, objects.
  function len(o: any): any { return o.length; }
  const o = { length: 99 };
  for (let i = 0; i < 64; i++) { len(o); len("abc"); len([1, 2]); }
  show("object-length", len(o));
  show("string-length", len("a heap string longer than sso"));
  show("sso-length", len("abc"));
  show("array-length", len([1, 2, 3, 4]));
}

// ---- rule 2: class ids never alias a ShapeId (#10824) --------------------------
{
  function F(this: any) {}
  class X extends (F as any) { }
  const first: any = { a: "first-shape" };
  prime(first);
  const x: any = new X();
  show("extends-function-instance", read(x));
  x.a = "x-own";
  show("extends-function-own", read(x));
  show("first-still-hits", read(first));
}

// ---- Array subclass: object-backed, reshapes on every push ------------------------
{
  class A extends Array<number> { a = "sub"; }
  const s: any = new A();
  prime(s);
  s.push(1); s.push(2);
  show("array-subclass-after-push", read(s));
  show("array-subclass-length", s.length);
  s.a = "sub2";
  show("array-subclass-rewrite", read(s));
}

// ---- `in` presence cache: a resolved-but-unarmed site must not match `{}` -----------
{
  function has(o: any): boolean { return "k" in o; }
  const proto = { k: 1 };
  const child = Object.create(proto);
  const got: boolean[] = [];
  for (let i = 0; i < 16; i++) got.push(has(child));
  got.push(has({}));
  got.push(has(Object.create(null)));
  got.push(has({ k: undefined }));
  got.push(has([]));
  console.log("in-presence", JSON.stringify(got));
}

// ---- #10595 twin: inherited accessor reading a shadowed field ---------------------
{
  class Base { tag = "base-tag"; }
  Object.defineProperty(Base.prototype, "tagViaGetter", {
    get() { return (this as any).tag; },
  });
  class Sub extends Base { tag = "sub-tag"; }
  const sub: any = new Sub();
  show("shadowed-direct", sub.tag);
  show("shadowed-via-getter", sub.tagViaGetter);
  const key = "tag";
  show("shadowed-computed", sub[key]);
  show("shadowed-in", key in sub);
}

console.log("done");
