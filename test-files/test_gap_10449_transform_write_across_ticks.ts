// #10449: a flowing PassThrough / options-object Transform emitted 'end' (and
// re-delivered a buffered chunk) as soon as its buffer drained, although
// `end()` had not been called, so every later write failed with
// ERR_STREAM_WRITE_AFTER_END. Producers that write across event-loop turns
// (nodemailer's MimeNode, mongodb's socket.pipe(Transform) framer) lost data.
import { PassThrough, Transform } from "node:stream";

function run(
  label: string,
  mk: () => any,
  gap: (f: () => void) => void,
  listenerFirst: boolean,
): Promise<void> {
  return new Promise((resolve) => {
    const s = mk();
    let out = "";
    const attach = () => {
      s.on("data", (c: any) => (out += String(c) + "|"));
      s.on("end", () => {
        console.log(label, "end", JSON.stringify(out), s.writableEnded, s.readableEnded);
        resolve();
      });
    };
    s.on("error", (e: any) => {
      console.log(label, "error", e.code);
      resolve();
    });
    if (listenerFirst) attach();
    s.write("a");
    if (!listenerFirst) attach();
    gap(() => {
      console.log(label, "mid", s.writableEnded, s.readableEnded);
      s.write("b");
      gap(() => s.end("c"));
    });
  });
}

const gaps: Array<[string, (f: () => void) => void]> = [
  ["immediate", (f) => setImmediate(f)],
  ["timeout", (f) => setTimeout(f, 5)],
  ["microtask", (f) => queueMicrotask(f)],
];
const mks: Array<[string, () => any]> = [
  ["PassThrough", () => new PassThrough()],
  [
    "Transform",
    () =>
      new Transform({
        transform(c: any, _e: any, cb: any) {
          cb(null, c);
        },
      }),
  ],
];

// The mongodb framer shape: one reply per event-loop gap, object-mode output.
class Framer extends Transform {
  constructor() {
    super({ readableObjectMode: true });
  }
  _transform(chunk: any, _enc: string, cb: (e?: Error | null) => void) {
    this.push("re:" + chunk.toString());
    cb();
  }
}

(async () => {
  for (const [mn, mk] of mks)
    for (const [gn, g] of gaps)
      for (const lf of [true, false]) await run(`${mn}/${gn}/${lf ? "first" : "after"}`, mk, g, lf);

  const framer = new Framer();
  framer.on("error", (e) => console.log("framer error", e.message));
  for (const m of ["a", "b", "c"]) {
    const got = new Promise((r) => framer.once("data", r));
    setTimeout(() => framer.write(Buffer.from(m)), 5);
    console.log("framer got", await got);
  }
})();
