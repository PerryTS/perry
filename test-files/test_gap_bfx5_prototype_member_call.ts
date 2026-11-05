// A builtin method read through its prototype (`Array.prototype.slice`,
// `Object.prototype.hasOwnProperty`), called with `call`/`apply`/`bind` on
// its own family, an array-like, a primitive and a foreign receiver.
class Sym {
  sv: number;
  constructor(sv: number) {
    this.sv = sv;
  }
}

const has = Object.prototype.hasOwnProperty;
const map: any = {};
map.a = new Sym(1);
console.log(has.call(map, "a"), has.call(map, "b"), has.apply(map, ["a"]));
const own = has.bind(map);
console.log(own("a"), own("toString"));
if (has.call(map, "a")) console.log(map.a.sv);

const arr = [new Sym(2), new Sym(3)];
const s1 = Array.prototype.slice.call(arr);
console.log(s1.length, s1[0].sv, s1 === arr);
const slice = Array.prototype.slice;
const s2 = slice.apply(arr, [1]);
console.log(s2.length, s2[0].sv);
const s3 = slice.bind(arr, 0, 1)();
console.log(s3.length, s3[0].sv);

// An array-like and the `arguments` object.
const like: any = { length: 2, 0: "x", 1: "y" };
console.log(JSON.stringify(Array.prototype.slice.call(like)));
function args(..._xs: any[]) {
  return Array.prototype.slice.call(arguments, 1);
}
console.log(JSON.stringify(args(1, 2, 3)));
// A primitive receiver: ToObject of the string.
console.log(JSON.stringify(Array.prototype.slice.call("ab")));
console.log(String.prototype.toUpperCase.call("abc"));

// A foreign receiver: `Map.prototype.get` on an array throws.
try {
  Map.prototype.get.call(arr, 0);
  console.log("no throw");
} catch (e) {
  console.log("TypeError", e instanceof TypeError);
}
// `hasOwnProperty` accepts any object; on undefined it throws.
console.log(Object.prototype.hasOwnProperty.call([1], "0"), has.call(Sym, "prototype"));
try {
  has.call(undefined, "a");
} catch (e) {
  console.log("TypeError", e instanceof TypeError);
}
// An `Object.prototype` method read under a computed key.
const k = process.argv.length > 99 ? "valueOf" : "hasOwnProperty";
const f: any = ({} as any)[k];
try {
  f.call();
  console.log("no throw");
} catch (e) {
  console.log("TypeError", e instanceof TypeError);
}
console.log(f.call(map, "a"));
// A shadowed namespace.
function shadow() {
  const Array: any = { prototype: { slice: (x: any) => "own " + typeof x } };
  return Array.prototype.slice.call(null, 1);
}
console.log(shadow());
// A key the prototype does not declare.
(Array.prototype as any).bfx5Grab = function (this: any) {
  return this.length;
};
console.log((Array.prototype as any).bfx5Grab.call(arr));
delete (Array.prototype as any).bfx5Grab;
