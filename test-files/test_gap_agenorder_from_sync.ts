const events: string[] = [];
function mark(label: string, result: any) { events.push(label + ":" + result.value + ":" + result.done); }
function clock() {
  queueMicrotask(() => events.push("micro"));
  Promise.resolve().then(() => events.push("p1")).then(() => events.push("p2")).then(() => events.push("p3"));
}
function* rows() {
 events.push("start"); yield Promise.resolve(1); events.push("resume"); yield 2; events.push("complete");
}
async function main() {
 const it=rows(); clock();
 for await (const v of it) { events.push("loop:"+v); }
 console.log(events.join("|"));
}
main();
