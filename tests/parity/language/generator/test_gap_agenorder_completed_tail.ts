const events: string[] = [];
async function* rows() { await 0; yield 1; events.push("complete"); }
async function main() {
 const it=rows();
 const first=it.next().then(r=>events.push("first:"+r.value));
 let completed=0;
 const pending: any[]=[];
 for(let i=0;i<8192;i++) {
  const index=i;
  pending.push(it.next().then(r=>{ if(r.done) completed++; if(index===2) events.push("third:"+r.done); }));
 }
 queueMicrotask(()=>events.push("micro"));
 Promise.resolve().then(()=>events.push("p1")).then(()=>events.push("p2")).then(()=>events.push("p3"));
 await Promise.all(pending); await first;
 events.push("done:"+completed);
 console.log(events.join("|"));
}
main();
