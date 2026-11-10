const events: string[]=[];
async function* rows() {
 try { await 0; yield 1; events.push("resume"); }
 catch(e) { events.push("caught:"+e); yield 2; }
 finally { events.push("finally"); await 0; events.push("closed"); }
}
async function main() {
 const it=rows();
 const a=it.next().then(r=>events.push("a:"+r.value+":"+r.done));
 queueMicrotask(()=>events.push("micro"));
 Promise.resolve().then(()=>events.push("p1")).then(()=>events.push("p2"));
 const b=it.return(Promise.reject("bad")).then(r=>events.push("b:"+r.value+":"+r.done),e=>events.push("b!"+e));
 const c=it.next().then(r=>events.push("c:"+r.value+":"+r.done));
 await Promise.all([a,b,c]); console.log(events.join("|"));
}
main();
