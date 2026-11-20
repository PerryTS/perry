// A loop region that reads a field as a Number (`h += o.a`) on a lane that
// does not guarantee a Number for every object of its shape (an `any` class
// field, a literal field born a string): the region must still run, and every
// value that is NOT a Number must take the generic path, whether it is there
// at loop entry or written by JS the loop calls.
class C {
    a: any;
    b: any;
    c: any;
    e: any;
    d: any;
    constructor(a: any, b: any, c: any, e: any, d: any) {
        this.a = a;
        this.b = b;
        this.c = c;
        this.e = e;
        this.d = d;
    }
}

function sum1(n: number, o: any): any {
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        h += o.a;
    }
    return h;
}

function sum4(n: number, o: any): any {
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        o.d = k;
        const r0 = o.a;
        const r1 = o.b;
        const r2 = o.c;
        const r3 = o.e;
        h += r0 + r1 + r2 + r3;
    }
    return h;
}

const o = new C(1, 2, 4, 8, 16);
console.log("number", sum1(10, o), sum4(10, o), o.d);
for (const v of ["s", 2147483647, 2147483648, -0, NaN, 1.5, true, null, undefined, "7"]) {
    o.a = v;
    console.log("entry", String(v), sum1(5, o), sum4(3, o));
}
o.a = 3;
o.b = { valueOf() { return 100; } };
console.log("valueOf", sum4(4, o));
o.b = 2;

// JS the loop calls writes a non-Number into a field the region reads.
let pokes = 0;
function poke(x: any, k: number): void {
    pokes++;
    if (k === 3) x.a = "x";
    if (k === 6) x.a = 5;
}
function sumCall(n: number, x: any): any {
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        h += x.a;
        poke(x, k);
    }
    return h;
}
const p = new C(1, 2, 4, 8, 16);
console.log("call", sumCall(10, p), pokes, p.a);

// A literal whose field is born a string, later holding a Number.
const lit: any = { a: "a", b: "bb", c: "cccc", e: "eeee", d: "dddd" };
console.log("lit-string", sum1(4, lit), lit.a.length);
lit.a = 6;
console.log("lit-number", sum1(4, lit));
lit.a = 6 | 0;
console.log("lit-int", sum1(4, lit));

// A string field read through `.length` is a receiver, not a Number operand.
function lens(n: number, x: any): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        x.d = k;
        h += x.a.length;
    }
    return h;
}
console.log("length", lens(5, { a: "abc", b: "bb", c: "c", e: "e", d: "d" }));
console.log("length-class", lens(5, new C("abcd", 1, 2, 3, 4)));

// The receiver is young and the loop allocates, so a collection can move it
// between the guard and the reads.
function churn(n: number): any {
    const q = new C(1, 2, 4, 8, 16);
    const junk: any[] = [];
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        q.d = k;
        h += q.a + q.b;
        junk.push({ k, s: "v" + k });
        if (junk.length > 4096) junk.length = 0;
    }
    q.a = "end";
    return h + q.a + junk.length;
}
console.log("churn", churn(200000));

// A module-level receiver whose class names its birth shape statically.
const m: any = new C(1, 2, 4, 8, 16);
function sumStatic(n: number): any {
    let h = 0.0;
    for (let k = 0; k < n; k++) {
        m.d = k;
        const r0 = m.a;
        const r1 = m.b;
        const r2 = m.c;
        const r3 = m.e;
        h += r0 + r1 + r2 + r3;
    }
    return h;
}
console.log("static", sumStatic(10));
m.c = "c";
console.log("static-string", sumStatic(3));
m.c = 4;
console.log("static-back", sumStatic(10));
