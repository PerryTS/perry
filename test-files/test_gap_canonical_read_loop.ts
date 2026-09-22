function run(n: any, O: any) { let h=0; for(let k=0;k<n;k++) h+=O.a+O.b; return h; }
function store(n: any, O: any) { let h=0; for(let k=0;k<n;k++){O.c=1;h+=O.a+O.b;} return h; }
function erase(O: any) { delete O.a; }
function callDelete(n: any, O: any) { let h=0; for(let k=0;k<n;k++){erase(O);h+=O.a+O.b;} return h; }
function reassign(n: any, O: any) { let h=0; for(let k=0;k<n;k++){O={a:3,b:4};h+=O.a+O.b;} return h; }
console.log(run(7,{a:1,b:2}));
const grown:any={}; grown.a=1; grown.b=2;
console.log(run(7,grown));
console.log(store(7,{a:1,b:2}));
console.log(callDelete(7,{a:1,b:2}));
let hits=0;
const getter:any={get a(){hits++;return hits;},b:2};
console.log(run(7,getter),hits);
console.log(reassign(7,{a:1,b:2}));
console.log(run(7,5));
console.log(run(0,null));
const dictionary:any={};
for(let j=0;j<1100;j++) dictionary['x'+j]=j;
dictionary.a=1;dictionary.b=2;
console.log(run(7,dictionary));
console.log(run(4,{a:'x',b:2}));
let coercions=0;
const value:any={valueOf(){coercions++;return 10;}};
console.log(run(4,{a:value,b:2}),coercions);
console.log(run(4,new Proxy({a:1,b:2},{get(t,k){return k==='a'?10:Reflect.get(t,k);}})));
