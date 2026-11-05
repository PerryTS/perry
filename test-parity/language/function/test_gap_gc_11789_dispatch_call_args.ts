// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_SCHEDULE_SEED=11789 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=4 PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1
// #11789 sweep: static / super / promise dispatch. `C.m(a, b)` on a class
// static, `super.m(a, b)`, and the fused `Promise.resolve(v).then(f, g)` held
// an earlier argument (or the resolved value) in a register while the later
// handler / argument was built by a call that collects. Output must be
// byte-identical to node.

function churn(rounds: number): number {
  let n = 0;
  for (let r = 0; r < rounds; r++) {
    const a: any[] = new Array(16);
    for (let j = 0; j < 16; j++) a[j] = { j, r, s: "v" + j };
    n += a.length;
  }
  return n;
}
const big: any = 4294967301;

class C {
  static m(a: string, b: number) {
    return a + "/" + b;
  }
}
console.log(C.m("x" + String(big), churn(1500)));

class A {
  m(a: string, b: number) {
    return a + "/" + b;
  }
}
class B extends A {
  m(a: string, b: number) {
    return "B:" + super.m("x" + a + String(big), churn(1500));
  }
}
console.log(new B().m("q", 1));

const mk = (tag: string) => {
  churn(1500);
  return (v: any) => console.log(tag, v);
};
Promise.resolve("q" + String(big)).then(mk("fused"), mk("unused"));
