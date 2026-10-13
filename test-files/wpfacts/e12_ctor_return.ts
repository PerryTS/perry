// E12: a constructor that returns another object.
// WPFACTS: site .r off
class Ret {
  r: number;
  constructor(flag: boolean) {
    this.r = 1;
    if (flag) return { pad: 0, r: "other" } as any;
  }
}
function rd(x: Ret) { return x.r; }
console.log(rd(new Ret(false)), rd(new Ret(true)));
