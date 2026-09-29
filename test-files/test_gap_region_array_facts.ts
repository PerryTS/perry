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


// Facts broken BEFORE the loop. The bodies below run no calls, so the loop
// region forms (PERRY_REGION_DIAG=4 prints an array plan for each) and only
// the preheader guard stands between F-body and a wrong read. The body only
// counts the elements that read `undefined` and stays small, so the region
// pays for its copy; a property read of the element would run
// the tower and re-check the array every iteration.

function eightObjs(): any[] {
    return [{ v: 1 }, { v: 2 }, { v: 3 }, { v: 4 }, { v: 5 }, { v: 6 }, { v: 7 }, { v: 8 }];
}

// A holey array, and Array.prototype gets an index property before the loop:
// the hole at 5 reads the prototype's element.
function protoIndexBefore(n: number): number {
    const xs: any[] = [{ v: 1 }, { v: 2 }];
    xs.length = 8;
    (Array.prototype as any)[5] = { v: 50 };
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        if (o === undefined) h++;
    }
    delete (Array.prototype as any)[5];
    return h;
}

// The array's own prototype is replaced before the loop.
function ownProtoBefore(n: number): number {
    const xs: any[] = [{ v: 1 }, { v: 2 }];
    xs.length = 8;
    const p: any = Object.create(Array.prototype);
    p[6] = { v: 60 };
    Object.setPrototypeOf(xs, p);
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        if (o === undefined) h++;
    }
    return h;
}

// Reads past the capacity: the guard must refuse. JSON.parse allocates an
// array of exactly its length (capacity 8, `k & 15`); a literal gets the
// minimum capacity of 16 (`k & 31`).
function capacityJson(n: number): number {
    const xs: any[] = JSON.parse('[{"v":1},{"v":2},{"v":3},{"v":4},{"v":5},{"v":6},{"v":7},{"v":8}]');
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 15];
        if (o === undefined) h++;
    }
    return h;
}

function capacityPastMin(n: number): number {
    const xs: any[] = eightObjs();
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 31];
        if (o === undefined) h++;
    }
    return h;
}

// A hole written before the loop reads `undefined`; with a prototype element
// at the same index it reads that element.
function holeBefore(n: number): number {
    const xs: any[] = eightObjs();
    delete xs[3];
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        if (o === undefined) h++;
    }
    return h;
}

function holeProtoBefore(n: number): number {
    const xs: any[] = eightObjs();
    delete xs[2];
    (Array.prototype as any)[2] = { v: 20 };
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        if (o === undefined) h++;
    }
    delete (Array.prototype as any)[2];
    return h;
}

// The length shrinks before the loop: the vacated slots read `undefined`.
function shrinkBefore(n: number): number {
    const xs: any[] = eightObjs();
    xs.length = 3;
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        if (o === undefined) h++;
    }
    return h;
}

function popBefore(n: number): number {
    const xs: any[] = eightObjs();
    xs.pop();
    xs.pop();
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 7];
        if (o === undefined) h++;
    }
    return h;
}

// Not an array: an array-like object and a typed array.
function notArrayQuiet(xs: any, n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = xs[k & 1];
        if (o === undefined) h++;
    }
    return h;
}

const N = 50000;
// An index property on a prototype turns off the array fast paths for the
// rest of the process, so every case that sets one runs last.
console.log("plain", plain(objs(8), N));
console.log("varying", varying(objs(8), N));
console.log("grownInLoop", grownInLoop(objs(8), 200));
console.log("storedInLoop", storedInLoop(objs(8), 200));
console.log("holes", holes(200));
console.log("accessorMidLoop", accessorMidLoop(200));
console.log("shrinkMidLoop", shrinkMidLoop(200));
console.log("lengthGrowMidLoop", lengthGrowMidLoop(200));
console.log("boundTooBig", boundTooBig(200));
console.log("notArray", notArray(200));
console.log("allocInLoop", allocInLoop(objs(8), 200000));
console.log("moduleConst", moduleConst(N));
console.log("capacityJson", capacityJson(200));
console.log("capacityPastMin", capacityPastMin(200));
console.log("holeBefore", holeBefore(200));
console.log("shrinkBefore", shrinkBefore(200));
console.log("popBefore", popBefore(200));
console.log("notArrayQuiet obj", notArrayQuiet({ 0: { v: 5 }, 1: { v: 6 }, length: 2 }, 200));
console.log("notArrayQuiet f64", notArrayQuiet(new Float64Array([1.5, 2.5]), 200));
console.log("notArrayQuiet arr", notArrayQuiet([{ v: 5 }, { v: 6 }], 200));
console.log("ownProtoMidLoop", ownProtoMidLoop(200));
console.log("ownProtoBefore", ownProtoBefore(200));
console.log("protoMidLoop", protoMidLoop(200));
console.log("protoBefore", protoBefore(200));
console.log("protoIndexBefore", protoIndexBefore(200));
console.log("holeProtoBefore", holeProtoBefore(200));
