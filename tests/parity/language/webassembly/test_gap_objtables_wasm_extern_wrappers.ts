// External bindings are per wrapper; copying props cannot transfer a brand.
const memory = new WebAssembly.Memory({ initial: 1, maximum: 3 });
const global = new WebAssembly.Global({ value: 'i32', mutable: true }, 7);
const table = new WebAssembly.Table({ element: 'anyfunc', initial: 1 });
const memoryClone: any = { ...memory };
const globalClone: any = { ...global };
const tableClone: any = { ...table };
let last: any;
for (let i = 0; i < 40000; i++) last = { i, value: { i } };
console.log(memory instanceof WebAssembly.Memory, global instanceof WebAssembly.Global, table instanceof WebAssembly.Table);
console.log(memoryClone instanceof WebAssembly.Memory, globalClone instanceof WebAssembly.Global, tableClone instanceof WebAssembly.Table);
global.value = 19;
console.log(global.value, memory.grow(1), memory.buffer.byteLength, table.length, last.i);
