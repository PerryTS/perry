// DESIGN-p4l4 B3: a call that is no typed call of one body rejects.
// vr-expect: ua,ub REJECTED
function mk(a: number, b: number) {
  return { ua: a, ub: b };
}
function f1(u: { ua: number; ub: number }) {
  return u.ua;
}
function f2(u: { ua: number; ub: number }) {
  return u.ub * 2;
}
const handlers = [f1, f2];
let s = 0;
for (let i = 0; i < 2; i++) s += handlers[i](mk(i, i + 1));
console.log(s);
