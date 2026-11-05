import { readFileSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";
import { load } from "./generated/bindings";
import { load as loadStream } from "./generated-stream/bindings";
import { load as loadNested } from "./generated-nested/bindings";
import { load as loadCompound } from "./generated-compound/bindings";
import { load as loadPaused } from "./generated-paused/bindings";
declare function gc(): void;
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
const bytes = readFileSync(process.argv[2]);
const context = new AsyncLocalStorage<object>();
async function delay(): Promise<void> { await new Promise<void>(resolve => setTimeout(resolve, 1)); }
async function main(): Promise<void> {
  const first = load(bytes, {});
  const data = first.exports["test:producer/data"];
  const expected = {};
  context.enterWith(expected);
  const future = await data.make();
  check(typeof (future as any).then === "undefined", "future is nonthenable");
  gc();
  const owned = await future.read();
  check(context.getStore() === expected, "read preserves async context");
  gc();
  owned.dispose();
  check(data.drops() === 1, "owned payload destructor runs once");
  let twice = false;
  try { await future.read(); } catch (_) { twice = true; }
  check(twice, "future cannot be read twice");
  future.dispose();
  first.dispose();

  const temporary = load(bytes, {});
  const handle = await (await temporary.exports["test:producer/data"].make()).read();
  gc();
  handle.dispose();
  check(temporary.exports["test:producer/data"].drops() === 1, "temporary read stays alive through its observer");
  temporary.dispose();

  const abandoned = load(bytes, {});
  await (async () => { await abandoned.exports["test:producer/data"].make(); })();
  gc();
  await delay();
  gc();
  await delay();
  check(abandoned.exports["test:producer/data"].drops() === 1, "unread future GC closes its producer and payload");
  abandoned.dispose();

  const streaming = loadStream(readFileSync(process.argv[3]), {});
  const streamData = streaming.exports["test:producer/data"];
  const stream = await streamData.make();
  check(typeof (stream as any).then === "undefined", "stream is nonthenable");
  const chunk = await stream.read(2);
  check(!chunk.done && chunk.value.length === 1, "stream returns its final owned chunk before EOF");
  gc();
  if (!chunk.done) chunk.value[0].dispose();
  const end = await stream.read(2);
  check(end.done, "stream reaches EOF after its final chunk");
  check(streamData.drops() === 1, "stream payload destructor runs once");
  const unopened = await streamData.make();
  unopened.dispose();
  await delay();
  check(streamData.drops() === 2, "closing unread stream retires its queued payload");
  streaming.dispose();

  const nested = loadNested(readFileSync(process.argv[4]), {});
  const nestedData = nested.exports["test:producer/data"];
  const inner = await (await nestedData.make()).read();
  gc();
  const nestedItem = await inner.read();
  nestedItem.dispose();
  check(nestedData.drops() === 1, "nested future preserves owned payload");
  nested.dispose();

  const compound = loadCompound(readFileSync(process.argv[5]), {}, {fuel: 100000000});
  const compoundData = compound.exports["test:compound/data"];
  const payload = await (await compoundData.make()).read();
  check(payload.text === "héllo 🌍", "future Unicode payload");
  check(payload.bytes instanceof Uint8Array && payload.bytes.length === 3145865, "future large byte payload");
  for (let i = 0; i < payload.bytes.length; i++) {
    if (payload.bytes[i] !== ((i * 31 + 7) & 255)) throw new Error("future byte payload mismatch");
  }
  gc();
  payload.item.dispose();
  check(compoundData.drops() === 1, "compound future owns its nested resource");
  compound.dispose();

  let release: () => void = () => {};
  let entered = false;
  const paused = loadPaused(readFileSync(process.argv[6]), {"test:producer/gate": {
    wait: async () => { entered = true; await new Promise<void>(resolve => { release = resolve; }); }
  }});
  const pausedData = paused.exports["test:producer/data"];
  for (let iteration = 1; iteration <= 2; iteration++) {
    entered = false;
    const waiting = await pausedData.make();
    const pending = waiting.read();
    let cancelled = false;
    const observed = pending.catch(() => { cancelled = true; });
    await delay();
    check(entered, "producer really suspended inside async import");
    let duplicate = false;
    try { waiting.read(); } catch (_) { duplicate = true; }
    check(duplicate, "parallel reads on one capability are rejected");
    waiting.dispose();
    await observed;
    check(cancelled, "disposing a pending read rejects its observer");
    release();
    await delay();
    check(pausedData.drops() === iteration && pausedData.writes() === 0,
          "soft cancellation retires payload and leaves the component reusable");
  }
  const forgotten = await pausedData.make();
  await (async () => { forgotten.read(); })();
  await delay();
  gc();
  await delay();
  gc();
  await delay();
  let closed = false;
  try { forgotten.read(); } catch (_) { closed = true; }
  check(closed, "collecting an unobserved pending read closes it even while its handle remains live");
  release();
  await delay();
  check(pausedData.drops() === 3 && pausedData.writes() === 0,
        "GC cancellation retires the suspended payload exactly once");
  paused.dispose();
  console.log("PASS component readers: ownership, temporary await, context, GC cancellation");
}
main().catch(error => { console.error(error); process.exitCode = 1; });
