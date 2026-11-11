let closes=0;let throws=0;
function make(mode:string='ok'):any {let n=0;const it:any={next(){return {value:n++,done:false};},[Symbol.iterator](){return this;}};
if(mode!=='absent')it.return=function(v:any){closes++;if(mode==='error')throw 'close-error';if(mode==='primitive')return 1;return {value:v,done:true};};
if(mode==='bad')it.return=3;
if(mode==='throw-method')it.throw=function(v:any){throws++;return {value:v,done:true};};
return it;}
function* delegate(mode:string){yield* make(mode);}
for(const mode of ['ok','absent','bad','error','primitive','throw-method']) {
 for(const action of ['return','throw']) {closes=0;throws=0;const g:any=delegate(mode);g.next();try{const r=action==='return'?g.return('finish'):g.throw('body-error');console.log(mode,action,'ok',String(r.value),r.done,closes,throws);}catch(e:any){console.log(mode,action,typeof e==='string'?e:e.name,closes,throws);}}
}

let final=0;
function* guarded(mode:string){try{yield* make(mode);yield 'after';}catch(e:any){yield typeof e==='string'?e:e.name;}finally{final++;}}
for(const mode of ['ok','absent','error','primitive','throw-method']) {
 for(const action of ['return','throw']) {
  final=0;closes=0;throws=0;const g:any=guarded(mode);g.next();
  try{const r=action==='return'?g.return('finish'):g.throw('body-error');const later=g.next();console.log('guarded',mode,action,String(r.value),r.done,String(later.value),later.done,final,closes,throws);}catch(e:any){console.log('guarded-error',mode,action,typeof e==='string'?e:e.name,final,closes);}
 }
}
function* yieldingFinally(){try{yield* make('ok');}finally{yield 'cleanup';final++;}}
final=0;closes=0;const yf:any=yieldingFinally();yf.next();const yr=yf.return('finish');const yn=yf.next();console.log('yielding-finally',yr.value,yr.done,yn.value,yn.done,final,closes);
let rebound=0;
const open:any={next(){return {value:'initial',done:false};},return(v:any){closes++;return {value:'keep',done:false};},throw(v:any){throws++;return {value:'caught',done:false};},[Symbol.iterator](){return this;}};
function* openDelegate(){try{yield* open;}finally{rebound++;}}
const od:any=openDelegate();od.next();const or=od.return('finish');const ot=od.throw('error');console.log('not-done',or.value,or.done,ot.value,ot.done,rebound);

// A synchronous delegate's result is an object even when it is a Promise:
// yield* must read its own done/value properties without awaiting it.
const promised:any={
 next(){return {value:0,done:false};},
 return(v:any){return Promise.resolve({value:'awaited-return',done:true});},
 throw(v:any){return Promise.resolve({value:'awaited-throw',done:true});},
 [Symbol.iterator](){return this;}
};
function* promiseDelegate(){yield* promised;}
const promiseGen:any=promiseDelegate();promiseGen.next();
const promiseReturn=promiseGen.return('finish');const promiseThrow=promiseGen.throw('error');
console.log('sync-promise-result',String(promiseReturn.value),promiseReturn.done,String(promiseThrow.value),promiseThrow.done);

let doneReads=0,valueReads=0,nextReads=0;
const returned:any={get done(){doneReads++;return false;},get value(){valueReads++;return 'returned';}};
const thrown:any={get done(){doneReads++;return false;},get value(){valueReads++;return 'thrown';}};
const rawIterator:any={
 next(v:any){nextReads++;return nextReads===1?{value:'initial',done:false}:{value:v,done:true};},
 return(v:any){return returned;},throw(v:any){return thrown;},
 [Symbol.iterator](){return this;}
};
function* rawDelegate(){return yield* rawIterator;}
const rawGen:any=rawDelegate();rawGen.next();
const rawReturn=rawGen.return('finish'),rawThrow=rawGen.throw('error');
console.log('raw-results',rawReturn===returned,rawThrow===thrown,doneReads,valueReads);
const rawNext=rawGen.next('resume');console.log('raw-resume',rawNext.value,rawNext.done,nextReads);

let methodCalls=0,callGets=0;
function callableIterator(mode:string):any {
 const it:any={next(){return {value:1,done:false};},[Symbol.iterator](){return this;}};
 for(const name of ['return','throw']) {
  if(mode==='object')it[name]={call(){methodCalls++;return {done:true};}};
  else {
   it[name]=function(v:any){methodCalls++;return {value:v,done:true};};
   Object.defineProperty(it[name],'call',{get(){callGets++;throw 'poison';}});
  }
 }
 if(mode==='missing-throw')delete it.throw;
 return it;
}
function* callableDelegate(mode:string){yield* callableIterator(mode);}
for(const mode of ['object','function','missing-throw']) {
 for(const action of ['return','throw']) {
  methodCalls=0;callGets=0;const g:any=callableDelegate(mode);g.next();
  try{const r=action==='return'?g.return('finish'):g.throw('error');console.log('callability',mode,action,r.value,r.done,methodCalls,callGets);}
  catch(e:any){console.log('callability',mode,action,typeof e==='string'?e:e.name,methodCalls,callGets);}
 }
}

function* privateResume(){
 try{yield* {next(){return {value:'first',done:false};},return(){throw 'delegated-error';},[Symbol.iterator](){return this;}};}
 catch(e){yield 'caught-'+e;}
 yield 'tail';
}
const privateG:any=privateResume();console.log('private-next-start',privateG.next().value);
privateG.next=()=>{throw 'public-next-must-not-run';};
console.log('private-next-return',privateG.return('done').value);
const privateThrow:any=privateResume();privateThrow.next();privateThrow.next=()=>{throw 'public-next-must-not-run';};
console.log('private-next-throw',privateThrow.throw('sent').value);
