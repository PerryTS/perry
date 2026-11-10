const events: string[]=[];
async function* rows() { events.push("body"); try { yield 1; } finally { events.push("finally"); } }
async function main() {
 const it=rows(); let next: any;
 const value=Promise.resolve(3);
 Object.defineProperty(value,"constructor",{get(){
  events.push("ctor");
  next=it.next().then(r=>events.push("next:"+r.value+":"+r.done));
  throw "bad";
 }});
 const end=it.return(value).then(r=>events.push("end:"+r.done),e=>events.push("end!"+e));
 const after=it.next().then(r=>events.push("after:"+r.value+":"+r.done));
 Promise.resolve().then(()=>events.push("p1")).then(()=>events.push("p2"));
 await Promise.all([next,end,after]); console.log(events.join("|"));
}
main();
