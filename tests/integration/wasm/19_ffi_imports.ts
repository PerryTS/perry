// Test: FFI function imports are generated as WASM imports under "ffi" namespace
// When these functions are provided by the host, they should be callable from WASM.

declare function add_numbers(a: number, b: number): number;
declare function greet(name: number): void;

// Since we're testing in Node.js without a real FFI provider,
// we test that the WASM module declares the imports correctly.
// The functions will be provided as stubs in the test runner.
const result = add_numbers(3, 4);
console.log(result);
greet(42);
console.log("done");
