// Object model step 1: a key add on an `Object.create(literal)` object whose
// prototype holds the key as a non-writable property must throw (Object.assign
// sets with Throw = true), and must leave no own key behind.

const Fixed: any = { id: 1, name: "fixed" };
Object.defineProperty(Fixed, "id", { writable: false });
const Frozen: any = { tag: "t", n: 0 };
Object.freeze(Frozen);

function attempt(label: string, f: () => any) {
  try {
    const r = f();
    console.log(label, "ok", Object.keys(r).join(","));
  } catch (e: any) {
    console.log(label, "threw", e instanceof TypeError);
  }
}
for (let i = 0; i < 3; i++) {
  attempt("fixed-id", () => Object.assign(Object.create(Fixed), { other: i, id: i }));
  attempt("fixed-name", () => Object.assign(Object.create(Fixed), { name: "x" + i }));
  attempt("frozen-tag", () => Object.assign(Object.create(Frozen), { tag: "y" }));
  attempt("frozen-new", () => Object.assign(Object.create(Frozen), { fresh: i }));
}
const partial: any = Object.create(Fixed);
try {
  Object.assign(partial, { before: 1, id: 2, after: 3 });
} catch (e: any) {
  console.log("partial", Object.keys(partial).join(","), partial.id);
}

// The same through a plain key add on `const x = Object.create(P)`, in
// strict code, where a failed add throws.
function strictAdds() {
  "use strict";
  const r: any = Object.create(Fixed);
  try {
    r.id = 5;
    console.log("strict r.id stored", r.id);
  } catch (e: any) {
    console.log("strict r.id threw", e instanceof TypeError, Object.keys(r).length);
  }
  const f: any = Object.create(Frozen);
  try {
    f.tag = "z";
    console.log("strict f.tag stored", f.tag);
  } catch (e: any) {
    console.log("strict f.tag threw", e instanceof TypeError, Object.keys(f).length);
  }
}
strictAdds();
