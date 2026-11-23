// Object.assign returns ToObject(target) (ECMA-262 20.1.2.1 step 1): a
// primitive target becomes a new wrapper object of its kind, never the
// primitive. The facts once proved the result of `Object.assign(5, ...)` an
// int32 5 (typeof "number", `.x` null). Both lowerings: the HIR form and a
// value call of the builtin.
function f() {
  const r: any = Object.assign(5 as any, { x: 1 });
  return [typeof r, r.x, r.valueOf(), r === 5];
}
console.log(JSON.stringify(f()));

const assign: any = Object.assign;
function g() {
  const r: any = assign(7, { y: 2 });
  return [typeof r, r.y, r.valueOf() + 1, r === 7];
}
console.log(JSON.stringify(g()));

function h() {
  const r: any = Object.assign("ab" as any, { z: 3 });
  return [typeof r, r[0], r[1], r.length, r.z, r.valueOf(), r === "ab"];
}
console.log(JSON.stringify(h()));

function k() {
  const r: any = Object.assign(true as any, {});
  return [typeof r, r.valueOf(), r === true];
}
console.log(JSON.stringify(k()));

try {
  Object.assign(null as any, { x: 1 });
  console.log("no throw");
} catch (e: any) {
  console.log(e instanceof TypeError);
}
