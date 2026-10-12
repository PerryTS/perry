const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,6,1,96,1,127,1,127,3,2,1,0,5,3,1,0,1,7,17,2,6,109,101,109,111,114,121,2,0,4,103,114,111,119,0,0,10,8,1,6,0,32,0,64,0,11]);
const instance = new WebAssembly.Instance(new WebAssembly.Module(bytes));
const memory = instance.exports.memory as WebAssembly.Memory;
const grow = instance.exports.grow as (n:number)=>number;
const first = memory.buffer;
new Uint8Array(first)[0] = 37;
for(let i=0;i<40000;i++){const a={i, words:[i,i+1]};if(a.words[0]!==i)throw new Error('allocation');}
if(grow(1)!==1 || first.byteLength!==0 || memory.buffer.byteLength!==131072 || new Uint8Array(memory.buffer)[0]!==37)throw new Error('Wasm grow');
const second = memory.buffer;
if(memory.grow(1)!==2 || second.byteLength!==0)throw new Error('JS grow');
const third = memory.buffer;
if(grow(1)!==3 || third.byteLength!==0 || memory.buffer.byteLength!==262144)throw new Error('repeat grow');
const fresh = new WebAssembly.Memory({initial:1});
if(fresh.buffer.byteLength!==65536)throw new Error('fresh owner');
console.log('wasm-memory',new Uint8Array(memory.buffer)[0],memory.buffer.byteLength,fresh.buffer.byteLength);
