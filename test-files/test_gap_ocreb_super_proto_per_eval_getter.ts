// Every definition reads and validates its own superclass prototype once.
function mk(P:any){ return class extends P {} }
let reads=0;
const P:any=function(){}.bind(null);
Object.defineProperty(P,"prototype",{get(){reads++; return {tag:reads}},configurable:true});
const A=mk(P), B=mk(P);
console.log(reads, Object.getPrototypeOf(A.prototype).tag, Object.getPrototypeOf(B.prototype).tag); // 2 1 2
let events:string[]=[];
const Q:any=function(){}.bind(null);
Object.defineProperty(Q,"prototype",{get(){events.push("prototype"); return {}},configurable:true});
function key(){events.push("key"); return "m"}
function keyed(P:any){return class extends P { [key()](){} }}
keyed(Q); keyed(Q); console.log(events.filter(x=>x==="prototype").length, events.filter(x=>x==="key").length); // 2 2
function F(){};
const old=F.prototype; const C=mk(F); F.prototype={}; const D=mk(F);
console.log(Object.getPrototypeOf(C.prototype)===old, Object.getPrototypeOf(D.prototype)===F.prototype); // true true
