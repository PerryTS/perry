const events: string[] = [];
function mark(label: string, result: any) { events.push(label + ":" + result.value + ":" + result.done); }
function clock() {
  queueMicrotask(() => events.push("micro"));
  Promise.resolve().then(() => events.push("p1")).then(() => events.push("p2")).then(() => events.push("p3"));
}
async function* rows() { await 0; yield 1; events.push("unreachable"); }
async function main() {
 const it = rows(); const a = it.next().then(r => mark("a",r)); clock();
 const b = it.return(Promise.resolve(8)).then(r => mark("b",r));
 const c = it.next().then(r => mark("c",r)); await Promise.all([a,b,c]); console.log(events.join("|"));
}
main();
