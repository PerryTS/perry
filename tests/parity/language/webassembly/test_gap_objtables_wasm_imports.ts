// Each later import uses the same stable cell after the imports object moves.
const bytes = new Uint8Array([0,97,115,109,1,0,0,0,1,6,1,96,1,127,1,127,2,9,1,3,101,110,118,1,102,0,0,3,2,1,0,7,7,1,3,114,117,110,0,1,10,13,1,11,0,32,0,16,0,32,0,16,0,106,11]);
let calls = 0;
const imports = { env: { f(x: number) { calls++; for (let i=0;i<12000;i++) { const a={x:i}; if(a.x<0) throw new Error('bad'); } return x + 5; } } };
let instance: any = new WebAssembly.Instance(new WebAssembly.Module(bytes), imports);
const run = instance.exports.run;
instance = undefined;
console.log(run(7), run(11), calls);
