function show(label: string, xs: any[]) { console.log(label, xs.join(',')); }
show('block', [3,1,2,1].sort((a,b) => { return a-b; }));
show('if', [3,1,2,1].sort((a,b) => { if(a<b) return -1; if(a>b) return 1; return 0; }));
show('ternary', [3,1,2,1].sort((a,b) => a<b ? -1 : a>b ? 1 : 0));
show('reverse', [3,1,2,1].sort((a,b) => a>b ? -1 : a<b ? 1 : 0));
function order(a: any,b: any) { if(a<b) return -1; if(a>b) return 1; return 0; }
show('named', [3,1,2,1].sort(order));
show('call', [3,1,2,1].sort((a,b) => order(a,b)));
const closed = (a: any,b: any) => a<b ? -1 : a>b ? 1 : 0;
show('const-call', [3,1,2,1].sort((a,b) => closed(a,b)));
const project = (x: any) => x.k;
const objects = [{k:2,id:'a'},{k:1,id:'b'},{k:2,id:'c'}];
objects.sort((a,b) => project(a)-project(b));
show('keys-stable',objects.map(x=>x.id));
show('Number',[3,1,2,1].sort((a,b)=>Number(a)-Number(b)));
show('unary',[3,1,2,1].sort((a,b)=>+a - +b));
show('infinity', [Infinity,1,-Infinity,Infinity].sort((a,b)=>a<b?-1:a>b?1:0));
show('nan', [2,NaN,1].sort((a,b)=>a<b?-1:a>b?1:0));
console.log('negative-zero',[0,-0,1].sort((a,b)=>a-b).map(x=>Object.is(x,-0)?'-0':String(x)).join(','));
let work=0;
show('effects',[3,1,2].sort((a,b)=>{work++;return a-b;}));
console.log('called',work>0);
let coercions=0;
const boxed=[{valueOf(){coercions++;return 3;}},{valueOf(){coercions++;return 1;}}];
boxed.sort((a,b)=>Number(a)-Number(b));
console.log('coerced',coercions>0,Number(boxed[0]),Number(boxed[1]));
let reads=0;
const getters=[{get k(){reads++;return 3;}},{get k(){reads++;return 1;}}];
getters.sort((a,b)=>project(a)-project(b));
console.log('getters',reads>0,getters[0].k,getters[1].k);
try {
  [2,1].sort((a,b)=>late(a,b));
  console.log('tdz-missing');
} catch(e) { console.log('tdz',e instanceof ReferenceError); }
const late = (a: number,b: number)=>a-b;
show("lexical", ["2","10","1"].sort((a,b)=>a<b?-1:a>b?1:0));
show("numeric-strings", ["2","10","1"].sort((a,b)=>Number(a)-Number(b)));
try {
  [2n,1n].sort((a,b)=>+a - +b);
  console.log("bigint-missing");
} catch(e) { console.log("bigint",e instanceof TypeError); }

function forward(xs: any[], cmp: any) { return xs.sort((a,b)=>cmp(a,b)); }
show('formal-callee', forward([3,1,2],closed));
