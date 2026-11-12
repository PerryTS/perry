// A class with reservations (#14485): `b` and `c` are stored in opposite
// orders by the two arms, so `new Res(1)` adds `b` out of the layout's
// order and the runtime orders that object by its overflow. With the facts'
// order check off (`PERRY_FACTS_CTOR_ORDER=0`) `d` is claimed definite and
// its store bypasses the overflow: slot 3 holds a value the object's key
// list does not name, a real false slot claim the witness reports.
class Res {
  [k: string]: any;
  constructor(m: number) {
    this.a = 1;
    if (m === 0) { this.b = "b"; this.c = "c"; } else { this.c = "c"; this.b = "b"; }
    this.d = m;
  }
}
function readD(r: Res): any { return r.d; }
function readB(r: Res): any { return r.b; }
const r0 = new Res(0), r1 = new Res(1);
console.log(readD(r0), readB(r0), Object.keys(r0).join(","));
console.log(readD(r1), readB(r1), Object.keys(r1).join(","));
