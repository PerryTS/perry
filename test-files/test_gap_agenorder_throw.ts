const events: string[] = [];
function mark(label: string, result: any) { events.push(label + ":" + result.value + ":" + result.done); }
function clock() {
  queueMicrotask(() => events.push("micro"));
  Promise.resolve().then(() => events.push("p1")).then(() => events.push("p2")).then(() => events.push("p3"));
}
async function* rows() {
 try { await 0; yield 1; } catch(e) { events.push("caught:" + e); yield 2; }
 events.push("complete");
}
async function main() {
 const it = rows(); const a = it.next().then(r => mark("a",r)); clock();
 const b = it.throw("bad").then(r => mark("b",r));
 const c = it.next().then(r => mark("c",r)); await Promise.all([a,b,c]); console.log(events.join("|"));
}
main();
