// A static-call site on a subclass (`S.m()` where only the parent R declares
// `m`) memoizes BOTH classes' shapes: storing `R.m` transitions R's shape and
// leaves S's alone, so a guard that checked only S would keep calling the old
// body. The loop bound is not a compile-time constant, so the site is one
// site that is armed on the first call and hit on the next ones.
class R {
  static m() {
    return "R.m";
  }
}
class S extends R {}
class T extends S {}
const n = process.argv.length + 5;
const out: string[] = [];
for (let i = 0; i < n; i++) {
  if (i === 3) (R as any).m = function () { return "stored"; };
  if (i === 5) (S as any).m = function () { return "S.own"; };
  out.push(S.m() + "/" + T.m());
}
console.log(out.join(","));
