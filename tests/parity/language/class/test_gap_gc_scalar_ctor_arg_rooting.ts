// parity-env: PERRY_GC_SCHEDULE_SEED=3 PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1
// GC witness for a scalar-replaced `new`. A non-escaping `new Pair(a, b)`
// keeps no object: its fields become frame slots and the constructor is
// inlined. The arguments are evaluated first, then the class value is read for
// `new.target` and the field initializers run, and only then are the
// arguments stored into the constructor's parameter slots. An argument held
// in a register across that window is not a root: a moving minor inside a
// later argument or a field initializer leaves the parameter naming the
// from-space string, and `p.a.length` reads garbage.
function churn(n: number): number {
    let s = "";
    for (let i = 0; i < n; i++) {
        s = s + String.fromCharCode(97 + (i % 26));
        if (s.length > 40) s = s.slice(20);
    }
    return s.length;
}
class Pair {
    t: number = churn(150);
    a: string;
    b: number;
    constructor(a: string, b: number) {
        this.a = a;
        this.b = b;
    }
}
function run(k: number): number {
    const p = new Pair("head-" + k + "-" + "z".repeat(k + 3), churn(200));
    return p.a.length + p.b + p.t;
}
let total = 0;
for (let k = 0; k < 3; k++) total += run(k);
process.stdout.write("done " + total + "\n");
