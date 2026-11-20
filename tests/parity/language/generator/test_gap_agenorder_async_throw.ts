const events: string[] = [];
async function fail() { await 0; throw "bad"; }
async function main() {
 const done=fail().then(()=>events.push("unexpected"),e=>events.push("caught:"+e));
 queueMicrotask(()=>events.push("micro"));
 Promise.resolve().then(()=>events.push("p1")).then(()=>events.push("p2"));
 await done; console.log(events.join("|"));
}
main();
