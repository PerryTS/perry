// Array slice S3: element reads inside #11650 loop regions. The preheader
// checks the array once (guard word, prototype facts, static index bound
// against capacity) and F-body reads `base + 8 * idx`. Every case below
// changes one of those facts in the middle of a loop, or starts without it,
// and must read what node reads.

function rd(o: any): number {
    return o === undefined ? -1 : o.v;
}

function objs(n: number): any[] {
    const xs: any[] = [];
    for (let i = 0; i < n; i++) xs.push({ v: i + 1, d: 0 });
    return xs;
}

// The plain region: a parameter array, reads under `k & 7`.
function plain(xs: any[], n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
    }
    return h;
}

// A per-iteration receiver read from the array (the matrix `varying` form).
function varying(xs: any[], n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        o.d = k;
        h += o.v;
    }
    return h;
}

// Grown in the loop (push past capacity moves the elements; the binding
// still names the old array's address until it is followed).
function grownInLoop(xs: any[], n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        if (k === 5) {
            for (let i = 0; i < 40; i++) xs.push({ v: 1000 + i, d: 0 });
            xs[3] = { v: 500, d: 0 };
        }
    }
    return h;
}

// Stored in the loop: a replaced element must be read back.
function storedInLoop(xs: any[], n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        h += o.v;
        xs[(k + 1) & 7] = { v: k, d: 0 };
    }
    return h;
}

// Holes read `undefined`.
function holes(n: number): number {
    const xs: any[] = new Array(8);
    xs[0] = { v: 3 };
    xs[5] = { v: 7 };
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
    }
    return h;
}

// A hole becomes visible through Array.prototype mid-loop.
function protoMidLoop(n: number): number {
    const xs: any[] = [{ v: 1 }, { v: 2 }];
    xs.length = 8;
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        if (k === 9) (Array.prototype as any)[6] = { v: 100 };
    }
    delete (Array.prototype as any)[6];
    return h;
}

// Array.prototype already has an index property before the loop.
function protoBefore(n: number): number {
    (Array.prototype as any)[4] = { v: 40 };
    const xs: any[] = [{ v: 1 }];
    xs.length = 8;
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
    }
    delete (Array.prototype as any)[4];
    return h;
}

// setPrototypeOf on the array itself mid-loop.
function ownProtoMidLoop(n: number): number {
    const xs: any[] = [{ v: 1 }, { v: 2 }];
    xs.length = 8;
    const p: any = Object.create(Array.prototype);
    p[7] = { v: 70 };
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        if (k === 11) Object.setPrototypeOf(xs, p);
    }
    return h;
}

// An accessor element defined mid-loop.
function accessorMidLoop(n: number): number {
    const xs: any[] = objs(8);
    let calls = 0;
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        if (k === 4) {
            Object.defineProperty(xs, 2, {
                get() {
                    calls++;
                    return { v: 20 };
                },
            });
        }
    }
    return h * 1000 + calls;
}

// Length shrinks mid-loop (pop / length=): the vacated slots read undefined.
function shrinkMidLoop(n: number): number {
    const xs: any[] = objs(8);
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        if (k === 10) xs.pop();
        if (k === 20) xs.length = 3;
    }
    return h;
}

// Length grows mid-loop through `length =` (new holes).
function lengthGrowMidLoop(n: number): number {
    const xs: any[] = objs(4);
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        if (k === 6) xs.length = 8;
        if (k === 30) xs[6] = { v: 60 };
    }
    return h;
}

// The static bound exceeds the array: the guard refuses, the loop still reads.
function boundTooBig(n: number): number {
    const xs: any[] = objs(4);
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 15]);
    }
    return h;
}

// Not an array at all.
function notArray(n: number): number {
    const xs: any = { 0: { v: 5 }, 1: { v: 6 }, length: 2 };
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 1]);
    }
    return h;
}

// Allocation in the loop body between reads (a collection may move the
// array): each read must see the live elements.
function allocInLoop(xs: any[], n: number): number {
    let h = 0;
    const keep: any[] = [];
    for (let k = 0; k < n; k++) {
        h += rd(xs[k & 7]);
        keep.push({ k: k, s: "x" + k });
        if (keep.length > 64) keep.length = 0;
    }
    return h;
}

// A module-level const array.
const MOD: any[] = objs(8);
function moduleConst(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = MOD[k & 7];
        o.d = k;
        h += o.v;
    }
    return h;
}

const N = 50000;
console.log("plain", plain(objs(8), N));
console.log("varying", varying(objs(8), N));
console.log("grownInLoop", grownInLoop(objs(8), 200));
console.log("storedInLoop", storedInLoop(objs(8), 200));
console.log("holes", holes(200));
console.log("protoMidLoop", protoMidLoop(200));
console.log("protoBefore", protoBefore(200));
console.log("ownProtoMidLoop", ownProtoMidLoop(200));
console.log("accessorMidLoop", accessorMidLoop(200));
console.log("shrinkMidLoop", shrinkMidLoop(200));
console.log("lengthGrowMidLoop", lengthGrowMidLoop(200));
console.log("boundTooBig", boundTooBig(200));
console.log("notArray", notArray(200));
console.log("allocInLoop", allocInLoop(objs(8), 200000));
console.log("moduleConst", moduleConst(N));
