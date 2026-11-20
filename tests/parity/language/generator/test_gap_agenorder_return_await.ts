const events: string[] = [];
function mark(label: string, result: any) { events.push(label + ":" + result.value + ":" + result.done); }
function clock() {
  queueMicrotask(() => events.push("micro"));
  Promise.resolve().then(() => events.push("p1")).then(() => events.push("p2")).then(() => events.push("p3"));
}
let release: any;
const gate = new Promise<number>(resolve => { release = resolve; });
async function* rows() {
 try { events.push("start"); const v=await gate; events.push("awake:"+v); yield v; }
 finally { events.push("finally"); await 0; events.push("closed"); }
}
async function main() {
 const it=rows(); const a=it.next().then(r=>mark("a",r));
 const b=it.return(8).then(r=>mark("b",r)); clock();
 queueMicrotask(()=>{ events.push("release"); release(3); });
 await Promise.all([a,b]); console.log(events.join("|"));
}
main();
