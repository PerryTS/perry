// DESIGN-p4l4 §2.2: every identity route rejects the template (R4).
// vr-expect: s1,s2 REJECTED R4Identity(===)
// vr-expect: l1,l2 REJECTED R4Identity(==)
// vr-expect: w1,w2 REJECTED R4Identity(switch)
// vr-expect: o1,o2 REJECTED R4Identity(Object.is)
// vr-expect: i1,i2 REJECTED
function ms(n: number) {
  return { s1: n, s2: n + 1 };
}
function ml(n: number) {
  return { l1: n, l2: n + 1 };
}
function mw(n: number) {
  return { w1: n, w2: n + 1 };
}
function mo(n: number) {
  return { o1: n, o2: n + 1 };
}
function mi(n: number) {
  return { i1: n, i2: n + 1 };
}
const a = ms(1), b = ms(1);
console.log(a === b, a !== b, a === a);
const c = ml(1), d = ml(1);
console.log(c == d, c != d);
const e = mw(1), f = mw(2);
switch (e) {
  case f:
    console.log("f");
    break;
  default:
    console.log("default", e.w1);
}
const g = mo(1);
console.log(Object.is(g, mo(1)), Object.is(g, g));
const h = mi(3);
console.log([1, 2].includes(h as any), [h].indexOf(h));
