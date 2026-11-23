// Node's promise-returning APIs keep their state (the result promise, state
// arrays, listener closures, the stream being consumed) in closures of the
// runtime. Those captures are heap references in value slots; a collection
// between scheduling and completion must keep and move them. Every case
// allocates while its work is pending.
import { EventEmitter, once, on } from "node:events";
import { promisify } from "node:util";
import { Readable, Writable, Transform, PassThrough } from "node:stream";
import { pipeline, finished } from "node:stream/promises";
import { text, json, buffer, arrayBuffer } from "node:stream/consumers";
import * as assert from "node:assert";

function churn(n: number): number {
  let keep: any[] = [];
  let total = 0;
  for (let i = 0; i < n; i++) {
    const o = { i, s: "v" + i, a: [i, i + 1, i + 2] };
    if (i % 89 === 0) keep.push(o);
    if (keep.length > 64) keep = keep.slice(32);
    total += o.a.length;
  }
  return total + keep.length;
}

function tick(n: number): Promise<void> {
  return new Promise((resolve) => {
    const step = () => {
      churn(4000);
      if (--n <= 0) resolve();
      else setImmediate(step);
    };
    setImmediate(step);
  });
}

async function events(): Promise<void> {
  const em = new EventEmitter();
  const first = once(em, "ready");
  churn(40000);
  setImmediate(() => em.emit("ready", "a", { n: 1 }));
  console.log("once", JSON.stringify(await first));

  const failing = once(em, "never");
  setImmediate(() => em.emit("error", new Error("emitter failed")));
  try {
    await failing;
  } catch (e) {
    console.log("once error", (e as Error).message);
  }

  const ac = new AbortController();
  const aborted = once(em, "never", { signal: ac.signal });
  churn(40000);
  setImmediate(() => ac.abort());
  try {
    await aborted;
  } catch (e) {
    console.log("once abort", (e as Error).name);
  }

  const seen: string[] = [];
  const stop = new AbortController();
  setImmediate(() => {
    for (let i = 0; i < 6; i++) em.emit("item", i, "x" + i);
    churn(40000);
    setImmediate(() => stop.abort());
  });
  try {
    for await (const args of on(em, "item", { signal: stop.signal })) {
      seen.push(args.join(":"));
      churn(8000);
    }
  } catch (e) {
    seen.push((e as Error).name);
  }
  console.log("on", seen.join(" "));

  const et = new EventTarget();
  const gone = new AbortController();
  const hits: string[] = [];
  et.addEventListener("ping", () => hits.push("signal"), { signal: gone.signal });
  et.addEventListener("ping", () => hits.push("plain"));
  churn(40000);
  et.dispatchEvent(new Event("ping"));
  gone.abort();
  churn(40000);
  et.dispatchEvent(new Event("ping"));
  console.log("EventTarget", hits.join(" "));
}

async function promisified(): Promise<void> {
  const cbStyle = (a: number, b: number, cb: (err: Error | null, sum?: number) => void) => {
    setImmediate(() => {
      churn(30000);
      if (a < 0) cb(new Error("negative"));
      else cb(null, a + b);
    });
  };
  const p = promisify(cbStyle);
  const results = await Promise.all([p(1, 2), p(3, 4), p(5, 6)]);
  let failed = "";
  try {
    await p(-1, 0);
  } catch (e) {
    failed = (e as Error).message;
  }
  console.log("promisify", JSON.stringify(results), failed);

  await assert.rejects(tick(2).then(() => Promise.reject(new TypeError("bad"))), TypeError);
  await assert.doesNotReject(tick(2));
  let rejects = "";
  try {
    await assert.rejects(tick(2));
  } catch (e) {
    rejects = (e as Error).name;
  }
  let doesNot = "";
  try {
    await assert.doesNotReject(tick(2).then(() => Promise.reject(new RangeError("r"))));
  } catch (e) {
    doesNot = (e as Error).name;
  }
  console.log("assert", rejects, doesNot);
}

async function streams(): Promise<void> {
  const chunks: string[] = [];
  const upper = new Transform({
    transform(chunk, _enc, cb) {
      churn(10000);
      cb(null, String(chunk).toUpperCase());
    },
  });
  const sink = new Writable({
    write(chunk, _enc, cb) {
      chunks.push(String(chunk));
      setImmediate(cb);
    },
  });
  await pipeline(Readable.from(["a", "b", "c", "d"]), upper, sink);
  console.log("pipeline", chunks.join(""));

  const broken = new Transform({
    transform(_chunk, _enc, cb) {
      churn(10000);
      cb(new Error("transform failed"));
    },
  });
  try {
    await pipeline(Readable.from(["a"]), broken, new PassThrough());
  } catch (e) {
    console.log("pipeline error", (e as Error).message);
  }

  const pt = new PassThrough();
  const done = finished(pt);
  churn(40000);
  pt.end("x");
  pt.resume();
  await done;
  console.log("finished", pt.writableFinished, pt.readableEnded);

  console.log("consumers text", await text(Readable.from(["he", "llo"])));
  console.log("consumers json", JSON.stringify(await json(Readable.from(['{"a":', "[1,2]}"]))));
  console.log("consumers buffer", (await buffer(Readable.from([Buffer.from("ab"), Buffer.from("cd")]))).toString());
  console.log("consumers arrayBuffer", (await arrayBuffer(Readable.from([Buffer.from("xyz")]))).byteLength);

  const source = () => Readable.from([1, 2, 3, 4, 5, 6, 7, 8]);
  const slow = async (x: number) => {
    await tick(1);
    return x;
  };
  console.log("map", JSON.stringify(await source().map(async (x: number) => (await slow(x)) * 2).toArray()));
  console.log("filter", JSON.stringify(await source().filter((x: number) => x % 2 === 0).toArray()));
  console.log("reduce", await source().reduce(async (acc: number, x: number) => acc + (await slow(x)), 0));
  console.log("some/every/find", await source().some((x: number) => x > 7), await source().every((x: number) => x > 1), await source().find((x: number) => x > 4));
  console.log("take/drop/flatMap", JSON.stringify(await source().drop(2).take(3).flatMap((x: number) => [x, -x]).toArray()));
  const each: number[] = [];
  await source().forEach(async (x: number) => {
    each.push(await slow(x));
  });
  console.log("forEach", each.sort((a, b) => a - b).join(","));
  const iterated: number[] = [];
  for await (const x of source()) {
    churn(5000);
    iterated.push(x as number);
  }
  console.log("for await", iterated.join(","));
}

async function webStreams(): Promise<void> {
  let n = 0;
  const rs = new ReadableStream<string>({
    async pull(controller) {
      await tick(1);
      if (n < 5) controller.enqueue("c" + n++);
      else controller.close();
    },
  });
  const ts = new TransformStream<string, string>({
    async transform(chunk, controller) {
      await tick(1);
      controller.enqueue(chunk + "!");
    },
  });
  const out: string[] = [];
  const ws = new WritableStream<string>({
    async write(chunk) {
      await tick(1);
      out.push(chunk);
    },
  });
  await rs.pipeThrough(ts).pipeTo(ws);
  console.log("pipeTo", out.join(" "));

  const [left, right] = new ReadableStream<number>({
    start(controller) {
      for (let i = 0; i < 4; i++) controller.enqueue(i);
      controller.close();
    },
  }).tee();
  const drain = async (s: ReadableStream<number>) => {
    const got: number[] = [];
    const reader = s.getReader();
    for (;;) {
      const { value, done } = await reader.read();
      churn(8000);
      if (done) break;
      got.push(value!);
    }
    return got.join(",");
  };
  console.log("tee", await drain(left), await drain(right));

  const writer = new WritableStream<string>({
    async write(chunk) {
      await tick(1);
      if (chunk === "bad") throw new Error("sink failed");
    },
  }).getWriter();
  await writer.write("ok");
  let failure = "";
  try {
    await writer.write("bad");
    await writer.close();
  } catch (e) {
    failure = (e as Error).message;
  }
  console.log("writer", failure);
}

async function main(): Promise<void> {
  await events();
  await promisified();
  await streams();
  await webStreams();
  console.log("done");
}

main().catch((e) => console.log("FAILED", e));
