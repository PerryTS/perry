// VR-2 S0 arm `vrswitch`: a record is a switch discriminant and a case.
function mk(n: number) {
  return { a: n, b: n > 1 };
}
const saved = mk(1);
const r = Math.random() > 2 ? saved : mk(2);
switch (r) {
  case saved:
    console.log("saved");
    break;
  default:
    console.log("other", r.a);
}
