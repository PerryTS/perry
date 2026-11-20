let calls=0; let steps=0;
function make(mode:string='ok'):any { let n=0; const it:any={next(){steps++;return {value:n++,done:n>3};},[Symbol.iterator](){return this;}};
 if(mode==='ok') it.return=function(){calls++;return {};};
 if(mode==='throw') it.return=function(){calls++;throw 'close-error';};
 if(mode==='primitive') it.return=function(){calls++;return 3;};
 if(mode==='bad') it.return=3;
 if(mode==='null') it.return=null;
 if(mode==='getter') Object.defineProperty(it,'return',{get(){calls++;throw 'get-error';}});
 return it;
}
function test(label:string,run:()=>any) { calls=0;steps=0; try {const r=run();console.log(label,'ok',String(r),calls,steps);}catch(e:any){console.log(label,typeof e==='string'?e:e.name,calls,steps);} }
for(const mode of ['ok','absent','null','bad','throw','primitive','getter']) {
 test('break-'+mode,()=>{for(const x of make(mode)) break;});
 test('return-'+mode,()=>{for(const x of make(mode)) return x;});
 test('throw-'+mode,()=>{for(const x of make(mode)) throw 'body-error';});
}
test('exhaustion',()=>{for(const x of make()) {} });
test('continue',()=>{for(const x of make()) continue;});
test('continue-own-label',()=>{here:for(const x of make()) continue here;});
test('outer-continue',()=>{outer:for(let k=0;k<2;k++){for(const x of make()) continue outer;}});
test('outer-break',()=>{outer:for(let k=0;k<2;k++){for(const x of make()) break outer;}});
test('nested-break',()=>{for(const x of make()){for(let k=0;k<1;k++)break;}});
test('caught-body-throw',()=>{for(const x of make()){try{throw 'caught';}catch(e){}}});
test('return-expression',()=>{for(const x of make('throw')) return (()=>{throw 'operand-error';})();});
let closes=0;
function* gen(){try{yield 1;yield 2;}finally{closes++;}}
closes=0;for(const x of gen()) break;console.log('generator-top-break',closes);
closes=0;try{for(const x of gen())throw 'body';}catch(e){}console.log('generator-top-throw',closes);
closes=0;outer:for(let k=0;k<2;k++){for(const x of gen())continue outer;}console.log('generator-top-label',closes);
function genReturn(){for(const x of gen())return x;}
closes=0;console.log('generator-fn-return',genReturn(),closes);
function genBreak(){for(const x of gen())break;}
closes=0;genBreak();console.log('generator-fn-break',closes);
function genThrow(){try{for(const x of gen())throw 'body';}catch(e){}}
closes=0;genThrow();console.log('generator-fn-throw',closes);

let bindingClosed=0;
function failBinding(){throw 'binding-error';}
function* bindingGen(){try{yield undefined;yield 1;}finally{bindingClosed++;}}
try{for(const [v=failBinding()] of (function*(){try{yield [undefined];}finally{bindingClosed++;}})()) {}}catch(e){console.log('generator-binding-error-top',e,bindingClosed);}
function functionBinding(){try{for(const {x=failBinding()} of (function*(){try{yield {};}finally{bindingClosed++;}})()) {}}catch(e){console.log('generator-binding-error-fn',e,bindingClosed);}}
functionBinding();

function nestedThrow(){throw {marker:'body'};}
closes=0;try{for(const x of gen())nestedThrow();}catch(e:any){console.log('generator-nested-call-throw',e.marker,closes);}
closes=0;try{for(const x of gen()){Array.from({[Symbol.iterator](){return this;},next(){return {value:1,done:false};},return(){throw 'inner-close';}},()=>{throw 'mapper';});}}catch(e){console.log('generator-runtime-trap-throw',e,closes);}

// A Rust-side callback trap must catch before reaching an older cleanup pad.
closes=0;for(const x of gen()){
 try{Array.from({[Symbol.iterator](){return this;},next(){return {value:1,done:false};},return(){return {};}},()=>{throw 'caught-mapper';});}
 catch(e){console.log('generator-caught-runtime-trap',e,closes);}
}
console.log('generator-caught-trap-completion',closes);

const userProto:any={return(){calls++;return {};}};
let protoIter:any=Object.create(userProto);protoIter.next=()=>({value:1,done:false});protoIter[Symbol.iterator]=function(){return this;};
calls=0;for(const value of protoIter)break;console.log('prototype-return',calls);
protoIter.return=()=>{calls+=10;return {};};calls=0;for(const value of protoIter)break;console.log('own-return-wins',calls);

closes=0;try{Array.from([1],()=>{for(const value of gen())throw 'callback-body';});}
catch(e){console.log('generator-in-runtime-callback',e,closes);}

// A return callback can schedule work; the synchronous-program proof must
// still inspect that callback and keep the event loop.
const scheduledClose = {
  [Symbol.iterator]() { return this; },
  next() { return { done: false, value: 1 }; },
  return() { Promise.resolve().then(() => console.log("close-microtask")); return { done: true }; }
};
for (const v of scheduledClose) { break; }
console.log("close-scheduled");
