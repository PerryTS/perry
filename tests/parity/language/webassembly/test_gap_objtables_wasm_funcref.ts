// A table function's wrapper owns the native host funcref.
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,7,1,96,2,127,127,1,127,3,2,1,0,4,4,1,112,0,1,7,15,2,5,116,97,98,108,101,1,0,3,97,100,100,0,0,9,7,1,0,65,0,11,1,0,10,9,1,7,0,32,0,32,1,106,11]);
const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes));
const table = instance.exports.table as WebAssembly.Table;
const func = table.get(0);
for (let i = 0; i < 40000; i++) {
  const waste = {i, text: `funcref-${i}`, values: [i, i+1]};
  if (waste.values[0] !== i) throw new Error('allocation');
}
if (func(20,17) !== 37 || table.get(0)(30,7) !== 37) throw new Error('funcref lifetime');
const fresh = new WebAssembly.Table({element:'anyfunc', initial:1});
if (fresh.get(0) !== null) throw new Error('fresh owner');
console.log('wasm-funcref', func(20,17), fresh.get(0));
