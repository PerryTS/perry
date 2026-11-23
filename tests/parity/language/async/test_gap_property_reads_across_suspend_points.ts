// Property reads, writes and builtin-method calls before and after every kind
// of suspension: await (in try/finally and catch), yield, for-await, yield*
// delegation, async generators. Each resume is a new activation of the step
// function; reads after it must see the same objects and the same writes.
class Box {
  n = 1;
  tag = "box";
  get twice(): number { return this.n * 2; }
}
class Wide extends Box { extra = 7; }

const boxes: any[] = [new Box(), new Wide(), { n: 5, tag: "lit" }];
const key = (i: number) => (i % 2 === 0 ? "n" : "tag");
const tick = () => new Promise<void>((r) => setTimeout(r, 0));

function touch(o: any, label: string, log: string[]): void {
  const m = new Map<string, number>([["a", o.n]]);
  const s = new Set<string>([o.tag]);
  log.push(`${label}:${o.n}/${o.tag}/${o[key(o.n)]}/${o.twice ?? "-"}/${m.get("a")}/${s.has(o.tag)}`);
}

async function awaitInTryFinally(log: string[]): Promise<number> {
  const o = boxes[0];
  touch(o, "pre", log);
  try {
    await tick();
    o.n += 1;
    touch(o, "try", log);
    await Promise.resolve();
    if (o.n > 1) throw new Error("boom");
  } catch (e: any) {
    touch(o, "catch-pre", log);
    await tick();
    touch(o, `catch-post(${e.message})`, log);
  } finally {
    await tick();
    o.n += 10;
    touch(o, "finally", log);
  }
  return o.n;
}

function* gen(o: any): Generator<number, string, number> {
  touch(o, "g0", (globalThis as any).LOG);
  const got = yield o.n;
  o.n += got;
  touch(o, "g1", (globalThis as any).LOG);
  try {
    yield o.twice;
  } finally {
    touch(o, "gfin", (globalThis as any).LOG);
  }
  return o.tag;
}

function* outer(o: any): Generator<number, string, number> {
  touch(o, "outer0", (globalThis as any).LOG);
  const r = yield* gen(o);
  touch(o, `outer1(${r})`, (globalThis as any).LOG);
  yield* [o.n, o.extra ?? -1];
  return r + "!";
}

async function* agen(o: any): AsyncGenerator<number> {
  for (let i = 0; i < 3; i++) {
    touch(o, `ag${i}`, (globalThis as any).LOG);
    await tick();
    o.n += 1;
    yield o.n;
    touch(o, `ag${i}b`, (globalThis as any).LOG);
  }
}

async function* adeleg(o: any): AsyncGenerator<number> {
  touch(o, "ad0", (globalThis as any).LOG);
  yield* agen(o);
  touch(o, "ad1", (globalThis as any).LOG);
}

async function main(): Promise<void> {
  const log: string[] = [];
  (globalThis as any).LOG = log;
  log.push(`r1=${await awaitInTryFinally(log)}`);

  const it = outer(boxes[1]);
  let step = it.next();
  while (!step.done) {
    log.push(`y=${step.value}`);
    step = it.next(3);
  }
  log.push(`ret=${step.value}`);

  let sum = 0;
  for await (const v of adeleg(boxes[2])) {
    touch(boxes[2], `fa(${v})`, log);
    sum += v;
    await tick();
  }
  log.push(`sum=${sum}`);

  // Early exit from for-await runs the generator's return path.
  for await (const v of agen(boxes[0])) {
    touch(boxes[0], `early(${v})`, log);
    break;
  }
  for (const line of log) console.log(line);
}

await main();
