// Issue #76 — WebAssembly host runtime PoC integration test.
//
// Exercises three things at once:
//   1. `embedWasm("./fixtures/add.wasm")` compile-time intrinsic
//      (no runtime fs read; bytes baked into the binary).
//   2. `WebAssembly.validate(bytes)` host call.
//   3. Standard instantiate-result shape (`result.instance.exports`) — no
//      `--enable-wasm-runtime` flag needed because codegen sees the
//      WebAssembly usage and auto-links libperry_wasm_host.a.
//
// Embedded module is the canonical i32 add export, ~41 bytes:
//   (module (func (export "add") (param i32 i32) (result i32)
//                  local.get 0 local.get 1 i32.add))

const bytes = embedWasm("./fixtures/add.wasm");

if (!WebAssembly.validate(bytes)) {
  console.log("FAIL: validate returned false");
} else {
  const result = WebAssembly.instantiate(bytes);
  if (result === undefined || result === null || result.instance === undefined) {
    console.log("FAIL: instantiate returned undefined");
  } else {
    const r = result.instance.exports.add(2, 3);
    if (r === 5) {
      console.log("OK");
    } else {
      console.log("FAIL: expected 5, got " + r);
    }
  }
}
