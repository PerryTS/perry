// A worker reads and writes properties before and after await, yield,
// for-await and yield* while the main thread does the same: each thread's
// compiled code must keep using its own per-thread runtime state.
import { Worker, isMainThread, parentPort } from "node:worker_threads";

class Box {
  n = 1;
  tag = "box";
  get twice(): number { return this.n * 2; }
}
const tick = () => new Promise<void>((r) => setTimeout(r, 0));

function touch(o: any, label: string, log: string[]): void {
  const m = new Map<string, number>([["a", o.n]]);
  log.push(`${label}:${o.n}/${o.tag}/${o["n"]}/${o.twice}/${m.get("a")}`);
}

function* gen(o: any, log: string[]): Generator<number, number, number> {
  touch(o, "g0", log);
  o.n += yield o.n;
  touch(o, "g1", log);
  return o.n;
}

async function* agen(o: any, log: string[]): AsyncGenerator<number> {
  for (let i = 0; i < 2; i++) {
    await tick();
    o.n += 1;
    touch(o, `ag${i}`, log);
    yield o.n;
  }
}

async function run(who: string): Promise<string[]> {
  const log: string[] = [];
  const o = new Box();
  o.tag = who;
  touch(o, "pre", log);
  try {
    await tick();
    touch(o, "try", log);
  } finally {
    await Promise.resolve();
    touch(o, "finally", log);
  }
  function* outer(): Generator<number, number, number> {
    return yield* gen(o, log);
  }
  const it = outer();
  it.next();
  log.push(`ret=${it.next(4).value}`);
  for await (const v of agen(o, log)) {
    await tick();
    touch(o, `fa(${v})`, log);
  }
  return log;
}

if (isMainThread) {
  const w = new Worker(new URL(import.meta.url));
  const mine = run("main");
  const theirs = new Promise<string[]>((resolve) => w.once("message", resolve));
  const [a, b] = await Promise.all([mine, theirs]);
  for (const line of a) console.log("main", line);
  for (const line of b) console.log("worker", line);
  await w.terminate();
} else {
  parentPort!.postMessage(await run("worker"));
}
