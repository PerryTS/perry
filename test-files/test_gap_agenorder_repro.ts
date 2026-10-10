// State dispatch must keep await and generator delivery on Node's microtask order.
const events: string[] = [];
async function exact(value: number) { return new Uint8Array([value]); }
async function* rows() {
  try {
    for (let i = 0; i < 3; i++) {
      const bytes = await exact(i + 10);
      events.push("read:" + bytes[0]);
      if (i === 1) {
        try { await Promise.reject("bad"); }
        catch (error) { events.push("caught:" + error); }
      }
      yield bytes[0];
      events.push("resumed:" + i);
    }
  } finally {
    events.push("finally");
    await Promise.resolve(0);
    events.push("closed");
  }
}
async function main() {
  const iterator = rows();
  const first = iterator.next().then(result => events.push("first:" + result.value));
  queueMicrotask(() => events.push("micro:1"));
  const second = iterator.next().then(result => events.push("second:" + result.value));
  Promise.resolve().then(() => events.push("promise:1")).then(() => events.push("promise:2"));
  await Promise.all([first, second]);
  const end = await iterator.return(99);
  events.push("end:" + end.value + ":" + end.done);
  console.log(events.join("|"));
}
main();
