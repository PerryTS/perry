// Issue #1037: String.fromCharCode / fromCodePoint via mem_call dispatch.
// Pre-fix the WASM codegen emitted the snake_case bridge name
// `string_from_char_code`, which had no entry in __memDispatch, so the call
// fell through to __classDispatch(code, "string_from_char_code", []) and
// returned undefined. That made `s += String.fromCharCode(x)` append the
// string "undefined" (9 chars) every iteration — the 16-iter repro produced
// length 144 instead of 16.

let result = '';
const depth = 0;
for (let i = 0; i < 16; i++) {
  result += String.fromCharCode(depth);
}
console.log('len:', result.length);

// Direct call: `String.fromCharCode(65) === 'A'`.
console.log('A:', String.fromCharCode(65));

// fromCodePoint goes through the same StringFromCharCode HIR variant in WASM.
console.log('B:', String.fromCodePoint(66));

// Explicit `=` + `+` form (matrix variant from the issue thread).
let r3 = '';
for (let i = 0; i < 4; i++) { r3 = r3 + String.fromCharCode(65); }
console.log('eq-plus:', r3, r3.length);
