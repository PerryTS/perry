let calls=0,steps=0;
function make(mode:string='ok',limit:number=4):any {let n=0;const it:any={next(){steps++;if(mode==='next-throw')throw 'next-error';return {value:undefined,done:n++>=limit};},[Symbol.iterator](){return this;}};
if(mode!=='absent')it.return=function(){calls++;if(mode==='throw')throw 'close-error';if(mode==='primitive')return 1;return {};};
if(mode==='bad')it.return=3;
if(mode==='null')it.return=null;
return it;}
function test(label:string,run:()=>any){calls=0;steps=0;try{run();console.log(label,'ok',calls,steps);}catch(e:any){console.log(label,typeof e==='string'?e:e.name,calls,steps);}}
function fail(){throw 'default-error';}
for(const mode of ['ok','absent','null','bad','throw','primitive']) {
 test('binding-'+mode,()=>{const [a]=make(mode);});
 test('assignment-'+mode,()=>{let a;[a]=make(mode);});
 test('default-'+mode,()=>{const [a=fail()]=make(mode);});
 test('assign-default-'+mode,()=>{let a;[a=fail()]=make(mode);});
}
test('empty-pattern',()=>{const []=make();});
test('elision',()=>{const [,a]=make();});
test('exhausted',()=>{const [a,b]=make('ok',0);});
test('rest',()=>{const [...a]=make();});
test('binding-next-throw',()=>{const [a=fail()]=make('next-throw');});
test('assignment-next-throw',()=>{let a;[a]=make('next-throw');});
test('setter',()=>{const obj:any={set x(v:any){throw 'setter-error';}};[obj.x]=make('throw');});
let closed=0;
function* gen(){try{yield 1;yield 2;yield 3;}finally{closed++;}}
closed=0;const [a]=gen();console.log('generator-binding',a,closed);
closed=0;let b;[b]=gen();console.log('generator-assignment',b,closed);
closed=0;const []=gen();console.log('generator-empty',closed);

for (const part of ['done','value']) {
 function hostile():any {return {next(){return part==='done'?{get done(){throw 'step-error';}}:{done:false,get value(){throw 'step-error';}};},return(){calls++;return {};},[Symbol.iterator](){return this;}};}
 test('binding-'+part,()=>{const [v=fail()]=hostile();});
 test('assignment-'+part,()=>{let v;[v]=hostile();});
}
closed=0;const fresh:any=gen();const []=fresh;const after=fresh.next();console.log('empty-then-next',after.done,closed);
test('binding-rest-next-throw',()=>{const [v=1,...rest]=make('next-throw');});
test('assignment-rest-next-throw',()=>{let rest;[...rest]=make('next-throw');});
test('nested-pattern',()=>{const [[v]]=make();});

let yielded=0;
function* finite(){try{yielded++;yield 10;yielded++;yield 20;yielded++;yield 30;}finally{closed++;}}
closed=0;const [one]=finite();console.log('generator-stops',one,yielded,closed);

test('binding-rest-only-next-throw',()=>{const [...[v]]=make('next-throw');});
