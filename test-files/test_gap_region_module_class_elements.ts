// Element-only COUNTER reads of module const arrays of class instances.
// Each slice is a fresh young array immediately before its loop: the setup
// loop may have tenured the original array under the schedule instrument.
// With PERRY_GC_INSTRUMENTS=1 at compilation, run with
// PERRY_GC_SCHEDULE_SEED=1 PERRY_GC_SCHEDULE_RATE=1
// PERRY_GC_SCHEDULE_ALLOC_KB=0 PERRY_GC_PROTECT_FROMSPACE=1.
// quiet has no body call: its poll refresh is the only way to update the
// cached element base after collection. Dropping emit_poll_refresh's base
// store must fail this test under that stress (or gc-root-dominance).
class Cell {
    value: number;
    touched: number;
    constructor(value: number) {
        this.value = value;
        this.touched = -1;
    }
}

function cells(n: number): Cell[] {
    const xs: Cell[] = [];
    for (let i = 0; i < n; i++) xs.push(new Cell(i + 1));
    return xs;
}

const QUIET: Cell[] = cells(256).slice();
function quiet(n: number): number {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        const o: Cell = QUIET[i];
        sum += o.value;
        o.touched = i;
    }
    return sum;
}
console.log("quiet", quiet(256), QUIET[0].touched, QUIET[255].touched);

// Identity checks consume every loaded element without a collecting body
// call. The region stays valid across the poll. Comparing with the separately
// rooted class instance catches poisoned edge fragments as well as protected
// pages; an undefined-only check could accidentally accept a poison word.
const POLL_EXPECTED: Cell = new Cell(17);
const POLL_ONLY: any[] = [
    POLL_EXPECTED, POLL_EXPECTED, POLL_EXPECTED, POLL_EXPECTED,
    POLL_EXPECTED, POLL_EXPECTED, POLL_EXPECTED, POLL_EXPECTED,
].slice();
function pollOnly(n: number): number {
    let seen = 0;
    for (let i = 0; i < n; i++) {
        const o: any = POLL_ONLY[i & 7];
        if (o !== POLL_EXPECTED) return -1;
        seen++;
    }
    return seen;
}
const witnessed = pollOnly(255.5);
if (witnessed !== 256) throw new Error("stale module array element after poll");
console.log("pollOnly", witnessed);

// An allocating call in the body stales the region; later accesses and
// the next iteration must re-check. All results still come from the objects.
function churn(n: number): number {
    const xs: Cell[] = cells(n);
    return xs.length;
}
const CALLS: Cell[] = cells(256).slice();
function collecting(n: number): number {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        const o: Cell = CALLS[i];
        sum += o.value;
        if (i === 127) sum += churn(512);
        o.touched = i;
        sum += o.value;
    }
    return sum;
}
console.log("collecting", collecting(256), CALLS[0].touched, CALLS[255].touched);

// A module let stays ineligible: a call can replace its binding.
let MUTABLE: Cell[] = cells(256).slice();
function replace(): void {
    MUTABLE = cells(256).slice();
    MUTABLE[128].value = 1000;
}
function changing(n: number): number {
    let sum = 0;
    for (let i = 0; i < n; i++) {
        const o: Cell = MUTABLE[i];
        sum += o.value;
        if (i === 127) replace();
    }
    return sum;
}
console.log("changing", changing(256), MUTABLE[128].value);
