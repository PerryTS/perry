// A named property read off a Number receiver is a read of Number.prototype
// (undefined for an absent key), never a dereference of the Number's bits.
// The shape safe-regex2 walks: ret's char tokens carry a Number `value`, and
// `node.value?.options` reads a key off it.
function walk(node: any): string {
  const options = node.options || node.value?.options;
  const stack = node.stack || node.value?.stack;
  return [typeof options, typeof stack].join(",");
}
console.log(walk({ type: 7, value: 47 }));
console.log(walk({ type: 1, stack: [] }));
console.log(walk({ type: 2, value: { options: [1] } }));

function read(x: any): unknown {
  return x.options;
}
for (const v of [47, 0.5, -3, 1e300, -0, NaN, Infinity]) console.log(String(read(v)));
function fixed(x: any): unknown {
  return x.toFixed;
}
console.log(typeof fixed(47), typeof fixed(1e300));

// Words with no bits above the 48-bit payload (0, denormals) and the other
// primitive kinds take the same named read.
for (const v of [0, -0, 5e-324, -5e-324, 2.2250738585072014e-308, true, false, "str", 1n]) {
  console.log(String(read(v)));
}
function keyOf(x: any, k: string): unknown {
  return x[k];
}
console.log(String(keyOf(5e-324, "options")), typeof keyOf(0, "toFixed"), typeof keyOf(5e-324, "toFixed"));
for (const v of [null, undefined]) {
  try {
    read(v);
    console.log("no throw");
  } catch (e) {
    console.log(e instanceof TypeError);
  }
}
function del(x: any): boolean {
  return delete x.options;
}
for (const v of [47, 0, 5e-324, NaN, true, "s"]) console.log(del(v));
const sym = Symbol("s");
function delSym(x: any): boolean {
  return delete x[sym];
}
for (const v of [47, 0, 5e-324, 1e300]) console.log(delSym(v));
function getSym(x: any): unknown {
  return x[sym];
}
console.log(String(getSym(47)), String(getSym(5e-324)));
for (const v of [47, 0, 5e-324, true, -3]) console.log(JSON.stringify(Object.keys(v)));
