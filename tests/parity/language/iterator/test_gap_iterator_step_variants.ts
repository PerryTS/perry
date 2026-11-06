// All native advance variants share the same result sink.
for (const make of [
  () => [10,20].keys(),
  () => [10,20].entries(),
  () => new Map([[1,10],[2,20]]).keys(),
  () => new Map([[1,10],[2,20]]).entries(),
  () => new Set([10,20]).keys(),
  () => new Set([10,20]).entries(),
  () => new Uint8Array([10,20]).values(),
]) {
  const it:any = make();
  let text="";
  for (const x of it) text+=JSON.stringify(x)+"|";
  const [first,...rest]:any=make();
  console.log(text,JSON.stringify(first),JSON.stringify(rest));
}
// A Proxy's indexed reads remain observable, including mid-step protocol writes.
const trace:string[]=[];
let iterator:any;
const backing=new Proxy([10,20],{get(target,key,receiver){
  if(key==="length" || key==="0" || key==="1")trace.push(String(key));
  if(key==="0")iterator.return=function(){trace.push("return");return {done:true};};
  return Reflect.get(target,key,receiver);
}});
iterator=Array.prototype.values.call(backing);
for(const x of iterator){console.log("proxy",x);break;}
console.log(trace.join("|"));
