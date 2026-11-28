// A spread object literal whose parts suspend evaluates them in source
// order: keys, values, spreads and computed keys around each `yield` and
// `await` run before it suspends or after it resumes, never reordered.

const log: string[] = [];
function mark<T>(label: string, value: T): T {
  log.push(label);
  return value;
}
function take(): string {
  const out = log.join(",");
  log.length = 0;
  return out;
}

function* generated(): Generator<string, any, any> {
  return {
    a: mark("a", 1),
    ...mark("spread0", { s0: 0 }),
    ...(yield "first"),
    [mark("key", "k")]: mark("value", 2),
    [yield "second"]: yield "third",
    b: mark("b", 3),
  };
}

const it = generated();
console.log(JSON.stringify(it.next()), take());
const source = { get s1() { log.push("get s1"); return 1; } };
console.log(JSON.stringify(it.next(source)), take());
console.log(JSON.stringify(it.next("dynamic")), take());
const done = it.next(4);
console.log(done.done, JSON.stringify(done.value), take());

async function awaited(): Promise<any> {
  const p = Promise.resolve({ x: 1 });
  queueMicrotask(() => log.push("tick"));
  return {
    before: mark("before", 0),
    ...(await p),
    [mark("key", "computed")]: await mark("value", Promise.resolve(5)),
    after: mark("after", 9),
  };
}

awaited().then((value) => console.log(JSON.stringify(value), take()));
