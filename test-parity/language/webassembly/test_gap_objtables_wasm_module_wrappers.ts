// Module identity comes from its owned native state, never public props.
const empty = new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]);
const module = new WebAssembly.Module(empty);
const clone: any = { ...module };
let last: any;
for (let i = 0; i < 40000; i++) last = { i, value: { i } };
console.log(module instanceof WebAssembly.Module, clone instanceof WebAssembly.Module);
console.log(WebAssembly.Module.exports(module).length, WebAssembly.Module.imports(module).length, last.i);
let rejected = false;
try { WebAssembly.Module.exports(clone); } catch { rejected = true; }
console.log('module-owned', rejected, new WebAssembly.Module(empty) !== module);
