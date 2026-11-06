const events: string[] = [];
function mark(label: string, result: any) { events.push(label + ":" + result.value + ":" + result.done); }
function clock() {
  queueMicrotask(() => events.push("micro"));
  Promise.resolve().then(() => events.push("p1")).then(() => events.push("p2")).then(() => events.push("p3"));
}
async function* rows() {
 for (let i=0; i<4; i++) { await 0; events.push("read:"+i); yield i; events.push("resume:"+i); }
}
async function main() {
 const it=rows(); const manual=it.next().then(r=>mark("manual",r)); clock();
 for await (const v of it) { events.push("loop:"+v); }
 await manual; console.log(events.join("|"));
}
main();
