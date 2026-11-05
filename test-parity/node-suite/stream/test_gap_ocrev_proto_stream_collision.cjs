const {statSync}=require("node:fs");
const Stream=require("node:stream");
Object.getPrototypeOf(statSync("."));
class R extends Stream{on(e,f){return super.on(e,f)}}
console.log(Object.getPrototypeOf(R.prototype)===Stream.prototype);
const r=new R(); let sum=0;
console.log(r.on("x",n=>{sum+=n})===r);
r.emit("x",3); console.log(sum);
