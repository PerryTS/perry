// Regression test for #1049 instances 2/3.
//
// Pre-fix: `wrapForI64` always coerced small-int Number returns to BigInt,
// breaking every WASM import declared with a non-i64 return — most notably
// `mem_call` (f64 return) and the `*_i32` family (is_truthy, js_strict_eq,
// class_set_method dispatch, ...). Boot threw
// `TypeError: Cannot convert a BigInt value to a number` on the first
// `class_set_method` mem_call before any user code ran.
//
// This fixture exercises:
//   - class registration + method dispatch (mem_call → class_set_method)
//   - i64-arg class method call (`moveBy(5)`)
//   - i32-return import via `if` truthiness check
//   - object field re-read across method calls (`desiredColumn` cursor)
//
// Expected output matches `node --experimental-strip-types` byte-for-byte.
class Cursor {
    line: number;
    column: number;
    desiredColumn: number;
    constructor() {
        this.line = 0;
        this.column = 0;
        this.desiredColumn = 0;
    }
    moveBy(dx: number): void {
        this.column = this.column + dx;
        this.desiredColumn = this.column;
    }
}

const c = new Cursor();
c.moveBy(5);
if (c.column) {
    console.log("col=" + c.column + " desiredColumn=" + c.desiredColumn);
} else {
    console.log("col was zero");
}
