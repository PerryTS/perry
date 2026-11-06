// DEC 250: %GeneratorPrototype% is locked in a compiled program (level 2 is refused); tests/parity/expected holds Perry output.
function locked(f: () => void) { try { f(); } catch (e) { console.log("DEC 250: " + (e as Error).message); } }
function* g(){yield 1;yield 2;}
for(const level of [1,2]){
 for(const name of ["next","return"]){
  const it:any=g();let target:any=it;
  for(let i=0;i<level;i++)target=Object.getPrototypeOf(target);
  const old=Object.getOwnPropertyDescriptor(target,name);
  const original=it.next.bind(it);let calls=0;
  locked(()=>{Object.defineProperty(target,name,{configurable:true,writable:true,value:function(){
    calls++;return name==="next"?original():{done:true};
  }});});
  let seen="";for(const x of it){seen+=x;break;}
  console.log(level,name,seen,calls);
  if(old)locked(()=>{Object.defineProperty(target,name,old);});else delete target[name];
 }
}
