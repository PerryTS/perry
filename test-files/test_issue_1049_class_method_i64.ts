// Issue #1049 instances 2/3 regression — WASM-target class-method dispatch
// at the i64 import boundary.
//
// Pre-fix on `--target web`: the runtime's `wrapForI64` unconditionally
// coerced small-integer Number returns to BigInt, breaking every WASM import
// declared with a non-i64 return (mem_call returns f64; is_truthy / class_set_method
// / etc. return i32). Boot threw
//   TypeError: Cannot convert a BigInt value to a number
// on the first `class_set_method` mem_call inside `_start`, long before any
// user code ran. The CLI native path is unaffected (no WASM ABI boundary), so
// this file also serves as a byte-for-byte parity check with Node.
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
