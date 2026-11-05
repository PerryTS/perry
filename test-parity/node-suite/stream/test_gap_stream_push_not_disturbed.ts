// #11212: `push()` is the producer side and must not disturb a Readable.
// Node derives `readableDidRead` from `_readableState.dataEmitted` (set only
// when a chunk is handed to a consumer — a 'data' emission, which `read()`
// performs too) and `stream.isDisturbed(r)` from `readableDidRead ||
// readableAborted`. undici's `body.text()` rejects with "unusable" when a
// body it has only pushed into reports disturbed.
import { Readable, Writable, PassThrough, isDisturbed } from "node:stream";

function show(label: string, r: any): void {
  console.log(label, "isDisturbed", isDisturbed(r), "readableDidRead", r.readableDidRead);
}
const tick = () => new Promise<void>((res) => setImmediate(res));

async function main(): Promise<void> {
  // push / unshift only.
  const a = new Readable({ read() {} });
  show("fresh", a);
  a.push("hello");
  a.push(Buffer.from("world"));
  show("after push", a);
  a.unshift("x");
  show("after unshift", a);
  await tick();
  show("after a tick", a);

  // read(): null does not count, data does.
  const b = new Readable({ read() {} });
  console.log("read on empty:", b.read());
  show("after empty read", b);
  b.push("abc");
  console.log("read:", String(b.read()));
  show("after read", b);

  // 'data' listener: disturbed once a chunk is emitted, not at attach time.
  const c = new Readable({ read() {} });
  c.on("data", (chunk: any) => console.log("data:", String(chunk)));
  show("data listener attached", c);
  await tick();
  show("flowing, nothing pushed", c);
  c.push("one");
  await tick();
  show("after data emitted", c);

  // resume() alone.
  const d = new Readable({ read() {} });
  d.resume();
  await tick();
  show("resumed, nothing pushed", d);
  d.push("two");
  await tick();
  show("resumed, after push", d);

  // pipe.
  const e = new Readable({ read() {} });
  const sinkChunks: string[] = [];
  const sink = new Writable({
    write(chunk: any, _enc: any, cb: any) {
      sinkChunks.push(String(chunk));
      cb();
    },
  });
  e.pipe(sink);
  await tick();
  show("piped, nothing pushed", e);
  e.push("three");
  await tick();
  show("piped, after push", e);
  console.log("sink got:", sinkChunks.join(","));

  // async iterator.
  const f = new Readable({ read() {} });
  f.push("four");
  f.push(null);
  show("before iteration", f);
  const got: string[] = [];
  for await (const chunk of f) got.push(String(chunk));
  console.log("iterated:", got.join(","));
  show("after iteration", f);

  // destroy before end: aborted counts for isDisturbed, not readableDidRead.
  const g = new Readable({ read() {} });
  g.push("five");
  g.destroy();
  await tick();
  show("destroyed with unread data", g);

  // destroy after a clean end.
  const h = new Readable({ read() {} });
  h.push(null);
  h.resume();
  await new Promise<void>((res) => h.on("close", () => res()));
  show("ended and closed", h);

  // Web conversion only wraps the stream; it disturbs once the web side reads.
  const w = new Readable({ read() {} });
  w.push("six");
  w.push(null);
  const web = Readable.toWeb(w);
  show("after toWeb", w);
  const reader = (web as any).getReader();
  const first = await reader.read();
  console.log("web read:", first.done, String(Buffer.from(first.value)));
  show("after web read", w);

  // A PassThrough written to but never read.
  const p = new PassThrough();
  p.write("seven");
  await tick();
  show("passthrough written, unread", p);
}
main();
