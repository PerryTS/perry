// Lazy JSON arrays and runtime-born arrays are not plain births
// (DESIGN-p4l1 §2.3, REVIEW-p4l1 finding 3, R1, R7): reads of them in hot
// loops, a call site whose callee is either `Array` or `JSON.parse`, and
// `Reflect.construct(Array, ...)`.

function total(a: any, n: number): number {
  let s = 0;
  for (let r = 0; r < 200; r++) {
    for (let i = 0; i < n; i++) {
      const k = i;
      const v = a[k];
      s = s + (typeof v === "number" ? v : v === undefined ? 1000 : 1);
    }
  }
  return s;
}

// JSON arrays, nested ones too.
const parsed = JSON.parse("[[1,2],[3]]");
const row0 = parsed[0];
const row1 = parsed[1];
console.log("json", total(row0, 3), total(row1, 2), total(parsed, 3));
const flat = JSON.parse("[1,2,3,4,5,6,7,8]");
console.log("flat", total(flat, 8), flat.length);

// A regex match result: elements, `.index`, `.length`.
const m = "abc123def".match(/(\d)(\d)/);
if (m) console.log("match", m[0], m[1], m.index, m.length, total(m, 3));

// Object.keys, slice, map, concat results.
const o = { a: 1, b: 2, c: 3 };
const keys = Object.keys(o);
const sl = [4, 5, 6].slice(1);
const mp = [1, 2, 3].map((x) => x * 2);
const cc = [1].concat([2, 3]);
console.log("runtime", keys[1], total(sl, 3), total(mp, 3), total(cc, 4));

// R1: one call site, a callee that is either `Array` or `JSON.parse`.
function make(useArray: boolean): any {
  const f: any = useArray ? Array : JSON.parse;
  return f("[9,8,7]");
}
const viaArray = make(true);
const viaJson = make(false);
console.log("site", viaArray.length, viaArray[0], viaJson.length, viaJson[0], total(viaJson, 3));

// R7: Reflect.construct of Array with a NewTarget.
class Sub extends Array {}
const rc = Reflect.construct(Array, [3], Sub);
console.log("reflect", rc.length, rc instanceof Sub, rc[0] ?? "h");
const plainRc = Reflect.construct(Array, [1, 2]);
console.log("reflect2", plainRc.length, plainRc[1], total(plainRc, 2));
