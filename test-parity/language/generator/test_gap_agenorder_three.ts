const events: string[] = [];
function mark(label: string, result: any) { events.push(label + ":" + result.value + ":" + result.done); }
function clock() {
  queueMicrotask(() => events.push("micro"));
  Promise.resolve().then(() => events.push("p1")).then(() => events.push("p2")).then(() => events.push("p3"));
}
async function* rows() {
  for (let i = 0; i < 3; i++) { await 0; events.push("read:" + i); yield i; events.push("resume:" + i); }
}
async function main() {
  const it = rows();
  const a = it.next().then(r => mark("a", r));
  clock();
  const b = it.next().then(r => mark("b", r));
  const c = it.next().then(r => mark("c", r));
  const d = it.next().then(r => mark("d", r));
  await Promise.all([a,b,c,d]); console.log(events.join("|"));
}
main();
