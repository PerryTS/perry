const events: string[]=[];
async function* rows() { events.push("body"); try { yield 1; } finally { events.push("finally"); } }
async function main() {
 const it=rows();
 const a=it.return(Promise.reject("bad")).then(r=>events.push("a:"+r.done),e=>events.push("a!"+e));
 const b=it.next().then(r=>events.push("b:"+r.value+":"+r.done));
 const c=it.next().then(r=>events.push("c:"+r.value+":"+r.done));
 Promise.resolve().then(()=>events.push("p1")).then(()=>events.push("p2"));
 await Promise.all([a,b,c]); console.log(events.join("|"));
}
main();
