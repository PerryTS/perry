// Loop regions over MODULE-LEVEL const arrays. The region keeps the array element
// base in an unrooted slot and re-derives it from the module global at the loop
// poll while the region is valid. A young array is moved by the first minor
// collection that runs at a poll inside the loop, so every read after that
// poll depends on the refresh.
//
// Two kinds of case:
//  - quietHoles: an element-only loop with no call in the body. The only
//    collection point is the loop poll, so the refresh is the ONLY thing
//    keeping the base current. This is the runtime witness: under
//    PERRY_GC_INSTRUMENTS=1 PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1
//    PERRY_GC_PROTECT_FROMSPACE=1 a build whose poll does not re-store the
//    base reads retired from-space and faults (measured: segfault at the first
//    collection on such a build, MATCH on main).
//  - the rest (quietAlt included): loops that form a region but do not fault
//    when the refresh is dropped, either because the body can collect and the
//    region re-checks the array after each such call, or because of how the
//    loop lowers. They are the checker cases: gc-root-dominance includes this
//    file in its corpus (test_gap_region*), the region stores the base in an
//    unrooted slot before the loop, and the checker must discharge that
//    through the valid-flag correlation rather than report it.
//
// Every case declares its own module const right before its loop, so the
// array is still young when the loop starts (a const built early is tenured
// long before a late loop runs and never moves again, which proves nothing).

function objs(n: number): any[] {
    const xs: any[] = [];
    for (let i = 0; i < n; i++) xs.push({ v: i + 1, d: 0 });
    return xs;
}

function nums(n: number): number[] {
    const xs: number[] = [];
    for (let i = 0; i < n; i++) xs.push(i * 1.5);
    return xs;
}

function mk(k: number): any {
    return { k };
}

function eightObjs(): any[] {
    return [{ v: 1 }, { v: 2 }, { v: 3 }, { v: 4 }, { v: 5 }, { v: 6 }, { v: 7 }, { v: 8 }];
}

const N = 40000;

// Quiet element-only loops (quietHoles is the runtime witness, see the header).
const Q1: any[] = eightObjs();
Q1.pop();
Q1.pop();
function quietHoles(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = Q1[k & 7];
        if (o === undefined) h++;
    }
    return h;
}
console.log("quietHoles", quietHoles(N));

const Q2: any[] = eightObjs();
Q2.length = 5;
function quietAlt(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = Q2[k & 7];
        h += o === undefined ? 3 : 1;
    }
    return h;
}
console.log("quietAlt", quietAlt(N));

// Element-receiver read and field write.
const OBJS: any[] = objs(8);
function objectRead(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        const o: any = OBJS[k & 7];
        o.d = k;
        h += o.v;
    }
    return h;
}
console.log("objectRead", objectRead(N));

// Dense number reads.
const NUMS_A: number[] = nums(16);
function denseRead(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += NUMS_A[k & 15];
    }
    return h;
}
console.log("denseRead", denseRead(N));

// Dense number reads next to an allocating call.
const NUMS_B: number[] = nums(16);
function denseCall(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += NUMS_B[k & 15] + mk(k).k;
    }
    return h;
}
console.log("denseCall", denseCall(N));

// Dense number reads and stores into two module arrays.
const NUMS_C: number[] = nums(16);
const OUT: number[] = nums(16);
function denseStore(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        OUT[k & 15] = NUMS_C[(k + 3) & 15] + k * 0.25;
        h += OUT[(k + 7) & 15];
    }
    return h;
}
console.log("denseStore", denseStore(N));

// A typed array (never moved) next to a moving one.
const F64 = new Float64Array([0.5, 1.5, 2.5, 3.5]);
const NUMS_D: number[] = nums(16);
function typedAndDense(n: number): number {
    let h = 0;
    for (let k = 0; k < n; k++) {
        h += F64[k & 3] * NUMS_D[k & 15];
    }
    return h;
}
console.log("typedAndDense", typedAndDense(N));

console.log("objs", OBJS.map((o: any) => o.d).join(","));
console.log("out", OUT.join(","));
