const events: string[] = [];
async function* rows() { await 0; throw "bad"; }
async function main() {
 const it=rows();
 const a=it.next().then(r=>events.push("a:"+r.done),e=>events.push("a!"+e));
 queueMicrotask(()=>events.push("micro"));
 Promise.resolve().then(()=>events.push("p1")).then(()=>events.push("p2"));
 const b=it.next().then(r=>events.push("b:"+r.done));
 const c=it.throw("closed").then(r=>events.push("c:"+r.done),e=>events.push("c!"+e));
 const d=it.return(Promise.reject("returnbad")).then(r=>events.push("d:"+r.done),e=>events.push("d!"+e));
 await Promise.all([a,b,c,d]); console.log(events.join("|"));
}
main();
