// #11200 / #10911: an INHERITED static reached through a subclass must read the
// DECLARING class's captured bindings.
//
// CommonJS module bodies lower as function bodies, so a class there that reads
// a module-scope binding is a capture-carrying class: its captures live on the
// class object as a per-evaluation array. Static dispatch through a subclass
// (`Sub.make()` running `Base.make`) handed the capture read the SUBCLASS
// object, and the read took slot `index` of the subclass's own array -- laid
// out for the subclass's capture list, so it returned an unrelated binding.
// mongodb 7.5.0's `CursorResponse.make(bson)` saw `typeof isErrorResponse ===
// "object"` and `find().toArray()` threw `value is not a function`.
//
// The #10911 half: a top-level `class A extends factory()` inheriting a
// capturing static METHOD from the factory's class expression read the
// capture as `undefined` (a ClassRef carries no capture array, and a class
// expression registers no declaration snapshot).
import * as resp from "./fixtures/issue_11200_inherited_static_captures/responses.cjs";

function say(label: string, f: () => unknown) {
  try {
    console.log(label, String(f()));
  } catch (e: any) {
    console.log(label, "THREW", e?.constructor?.name, e?.message);
  }
}

const R: any = resp;

// 1. The mongodb shape: `make` on the base, called through each subclass.
say("Base.make", () => R.Base.make("abc").constructor.name);
say("Sub.make", () => R.Sub.make("abc").constructor.name);
say("Sub2.make", () => R.Sub2.make("abc").constructor.name);
say("Leaf.make", () => R.Leaf.make("abc").constructor.name);
say("Sub.make(err)", () => R.Sub.make("err").constructor.name);
// `(responseType ?? MongoDBResponse).make(bson)` -- a computed receiver.
for (const responseType of [undefined, R.Sub, R.Leaf]) {
  say("(rt ?? Base).make", () => (responseType ?? R.Base).make("xy").constructor.name);
}
say("Sub.make.elements", () => JSON.stringify(R.Sub.make("abcd").elements));

// 2. Module-scope function / namespace object / const table, read by an
//    inherited static, at every depth.
say("Base.describe", () => R.Base.describe());
say("Sub.describe", () => R.Sub.describe());
say("Sub2.describe", () => R.Sub2.describe());
say("Leaf.describe", () => R.Leaf.describe());

// 3. A static getter using `this` and a captured class, via subclasses.
say("Base.tag", () => R.Base.tag);
say("Sub.tag", () => R.Sub.tag);
say("Leaf.tag", () => R.Leaf.tag);

// 4. The subclasses' own capturing statics still read their own captures.
say("Sub.kind", () => R.Sub.kind());
say("Leaf.kind", () => R.Leaf.kind());
say("Leaf.leafOnly", () => R.Leaf.leafOnly());

// 5. Instances built through the inherited static keep working.
say("Leaf.make.more", () => R.Leaf.make("q").more);
say("Sub.make instanceof", () => R.Sub.make("q") instanceof R.Sub);

// 6. Inherited static via `.call` with a subclass receiver.
say("Base.make.call(Sub2)", () => R.Base.make.call(R.Sub2, "zz").constructor.name);
say("Base.describe.call(Leaf)", () => R.Base.describe.call(R.Leaf));

// 7. #10911: capturing statics of a factory class expression, inherited by
//    top-level declarations, including two levels down.
function fCapM(tag: string) {
  return class Out {
    static who() { return this; }
    static tagv() { return tag; }
    static get tagg() { return tag + ":" + this.name; }
  };
}
class A3 extends fCapM("a") {}
class B3 extends fCapM("b") {}
class C3 extends A3 {}
say("A3.who()===A3", () => (A3 as any).who() === A3);
say("A3.tagv()", () => (A3 as any).tagv());
say("B3.tagv()", () => (B3 as any).tagv());
say("C3.tagv()", () => (C3 as any).tagv());
say("A3.tagg", () => (A3 as any).tagg);
say("C3.tagg", () => (C3 as any).tagg);
const O = fCapM("o");
say("O.tagv()", () => (O as any).tagv());
say("A3.tagv() again", () => (A3 as any).tagv());

// 8. A capturing factory class whose subclass carries its own captures.
function fPair(left: string) {
  const helper = (s: string) => "<" + s + ">";
  return class P {
    static show() { return helper(left) + "@" + this.name; }
  };
}
function fChild(right: number) {
  const P = fPair("L" + right);
  return class Q extends P {
    static own() { return right * 2; }
  };
}
const Q1: any = fChild(1);
const Q2: any = fChild(2);
say("Q1.show()", () => Q1.show());
say("Q2.show()", () => Q2.show());
say("Q2.own()", () => Q2.own());
class Q3 extends Q2 {}
say("Q3.show()", () => (Q3 as any).show());
