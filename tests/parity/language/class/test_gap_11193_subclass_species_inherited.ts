// #11193: a user subclass of a built-in inherits the built-in's
// `get [Symbol.species]` accessor, and the getter answers the subclass itself.
// #11338 installed the accessor on `Array`, `Map`, … but `class X extends
// Array {}` still read `X[Symbol.species]` as `undefined`: the class ref's
// symbol lookup never reached the built-in constructor its chain ends in.
//
// Once the species is found, RegExp `split`/`matchAll` construct the matcher
// through it, i.e. through a dynamic construct of the subclass. For a subclass
// with no constructor of its own that construct produced a plain object with
// no `[[RegExpMatcher]]` (same for typed-array and ArrayBuffer subclasses), so
// the synthesized default constructor now runs the built-in's own Construct.

const S = Symbol.species;

class MyArray extends Array {}
class MyMap extends Map {}
class MySet extends Set {}
class MyPromise extends Promise<any> {}
class MyRegExp extends RegExp {}
class MyArrayBuffer extends ArrayBuffer {}
class MyU8 extends Uint8Array {}
class Deep extends MyArray {}
const Expr = class extends Array {};

const rows: [string, any][] = [
  ["MyArray", MyArray],
  ["MyMap", MyMap],
  ["MySet", MySet],
  ["MyPromise", MyPromise],
  ["MyRegExp", MyRegExp],
  ["MyArrayBuffer", MyArrayBuffer],
  ["MyU8", MyU8],
  ["Deep", Deep],
  ["Expr", Expr],
];
for (const [n, C] of rows) {
  console.log(
    n,
    "own:", Object.getOwnPropertyDescriptor(C, S) === undefined ? "none" : "own",
    "species is ctor:", C[S] === C,
    "typeof:", typeof C[S],
  );
}

// Through a dynamic receiver and Reflect.get's receiver argument.
const dyn: any = MyArray;
console.log("dynamic:", dyn[S] === MyArray, Reflect.get(Array, S, MyArray) === MyArray);

// The prototype does not see the constructor's statics.
console.log("prototype:", (MyArray.prototype as any)[S] === undefined);

// A subclass's own species wins over the inherited accessor.
class Override extends Array {
  static get [Symbol.species]() {
    return Array;
  }
}
class BelowOverride extends Override {}
console.log("override:", (Override as any)[S] === Array, (BelowOverride as any)[S] === Array);

// Inherited user statics are unchanged.
class Base {
  static get [Symbol.species]() {
    return Base;
  }
}
class Kid extends Base {}
console.log("user base:", (Kid as any)[S] === Base);

// The base constructors still answer themselves.
console.log("intrinsics:", (Array as any)[S] === Array, (Map as any)[S] === Map, (Uint8Array as any)[S] === Uint8Array);

// RegExp split / matchAll construct the matcher through the subclass species.
console.log("split:", "a,b,c".split(new MyRegExp(",")), "x1y2".split(new MyRegExp("\\d"), 1));
console.log("matchAll:", [..."a1b22".matchAll(new MyRegExp("\\d+", "g"))].map((m) => m[0]));

// Constructing an implicit-constructor subclass through a VALUE builds the
// exotic built-in, like the literal `new MyRegExp(…)` does.
class Tagged extends RegExp {
  tag = 7;
}
class DeepRegExp extends MyRegExp {}
const R: any = MyRegExp;
const r = new R(",", "g");
console.log("dyn RegExp:", r instanceof MyRegExp, r.flags, r.source, "a,b,c".replace(r, "+"));
const rr = Reflect.construct(MyRegExp, [r, "y"]);
console.log("reflect RegExp:", rr instanceof MyRegExp, rr.flags, rr.exec(",") !== null);
const T: any = Tagged;
const t = new T("b", "i");
console.log("fields:", t instanceof Tagged, t.flags, t.tag, t.test("ABC"));
const D: any = DeepRegExp;
const d = new D("z");
console.log("deep:", d instanceof DeepRegExp, d instanceof MyRegExp, d.test("xyz"));
const U: any = MyU8;
const u = new U(3);
console.log("dyn Uint8Array:", u instanceof MyU8, u.length, u.byteLength);
const B: any = MyArrayBuffer;
const b = new B(5);
console.log("dyn ArrayBuffer:", b instanceof MyArrayBuffer, b.byteLength);

// Promise then keeps building the subclass.
const p = MyPromise.resolve(1);
const next = p.then((v: number) => v + 1);
console.log("then is MyPromise:", next instanceof MyPromise);
next.then((v: number) => console.log("then value:", v));
