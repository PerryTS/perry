function* g(){yield 1;yield 2;}
for(const level of [1,2]){
 for(const name of ["next","return"]){
  const it:any=g();let target:any=it;
  for(let i=0;i<level;i++)target=Object.getPrototypeOf(target);
  const old=Object.getOwnPropertyDescriptor(target,name);
  const original=it.next.bind(it);let calls=0;
  Object.defineProperty(target,name,{configurable:true,writable:true,value:function(){
    calls++;return name==="next"?original():{done:true};
  }});
  let seen="";for(const x of it){seen+=x;break;}
  console.log(level,name,seen,calls);
  if(old)Object.defineProperty(target,name,old);else delete target[name];
 }
}
