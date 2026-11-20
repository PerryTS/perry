// A class with reservations (#14485): `b` and `c` are stored by two of the
// arms only (reservations), in opposite orders, so `new Res(1)` adds `b`
// out of the layout's order and the runtime orders that object by its
// overflow. With the facts'
// order check off (`PERRY_FACTS_CTOR_ORDER=0`) `d` is claimed definite and
// its store bypasses the overflow: slot 3 holds a value the object's key
// list does not name, a real false slot claim the witness reports.
class Res {
  [k: string]: any;
  constructor(m: number) {
    this.a = 1;
    if (m === 0) { this.b = "b"; this.c = "c"; } else if (m === 1) { this.c = "c"; this.b = "b"; }
    this.d = m;
  }
}
function readD(r: Res): any { return r.d; }
function readB(r: Res): any { return r.b; }
for (const m of [0, 1, 2]) {
  const r = new Res(m);
  console.log(readD(r), readB(r), Object.keys(r).join(","));
}
