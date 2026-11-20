// HWS companion: wasm_host
// HWS hook: js_runtime_install_wasm_host
// HWS prefixes: perry_runtime::webassembly::|perry_wasm_host::|wasmtime::
const Memory=WebAssembly.Memory;const memory=new Memory({initial:1,maximum:3});
console.log(memory.buffer.byteLength);console.log(memory.grow(1),memory.buffer.byteLength);
