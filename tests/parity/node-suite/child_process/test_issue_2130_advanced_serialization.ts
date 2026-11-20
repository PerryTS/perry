// #2130 — child_process.fork(..., { serialization: 'advanced' }): the IPC
// channel uses V8 structured-clone framing instead of newline JSON, so Buffers
// and TypedArrays keep their type/byte fidelity across the channel (plain JSON
// flattens a Buffer to { type:'Buffer', data:[...] }). The forked module runs
// under `node`, which speaks the same V8 wire format, so this is byte-for-byte
// vs `node --experimental-strip-types` when node is on PATH.
import * as cp from "node:child_process";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// Echo child: reports the *type fidelity* of what it received, then disconnects
// on 'bye'. Replies travel back over the same advanced channel.
const childSrc = [
  "process.on('message', (m) => {",
  "  if (m && m.cmd === 'buf') {",
  "    process.send({ kind: 'buf', isBuf: Buffer.isBuffer(m.payload), len: m.payload.length, first: m.payload[0], last: m.payload[m.payload.length - 1] });",
  "  } else if (m && m.cmd === 'large') {",
  "    let sum = 0; for (const b of m.payload) sum += b;",
  "    process.send({ kind: 'large', isBuf: Buffer.isBuffer(m.payload), len: m.payload.length, sum });",
  "  } else if (m && m.cmd === 'mixed') {",
  "    const o = m.payload;",
  "    process.send({ kind: 'mixed', n: o.n, s: o.s, b: o.b, arrLen: o.arr.length, arrSum: o.arr.reduce((a, x) => a + x, 0), nestedHi: o.nested.hi, isU8: o.u8 instanceof Uint8Array, u8sum: Array.from(o.u8).reduce((a, x) => a + x, 0) });",
  "  } else if (m && m.cmd === 'bye') {",
  "    process.disconnect();",
  "  }",
  "});",
].join("\n");
const childPath = path.join(
  os.tmpdir(),
  "perry_adv_child_" + process.pid + ".mjs",
);
fs.writeFileSync(childPath, childSrc);

const child = cp.fork(childPath, [], { serialization: "advanced" });
console.log("connected initially:", child.connected);

await new Promise<void>((resolve) => {
  const replies: any[] = [];
  child.on("message", (m: any) => {
    replies.push(m);
    if (m.kind === "buf") {
      console.log(
        `buf -> isBuf=${m.isBuf} len=${m.len} first=${m.first} last=${m.last}`,
      );
    } else if (m.kind === "large") {
      console.log(`large -> isBuf=${m.isBuf} len=${m.len} sum=${m.sum}`);
    } else if (m.kind === "mixed") {
      console.log(
        `mixed -> n=${m.n} s=${m.s} b=${m.b} arrLen=${m.arrLen} arrSum=${m.arrSum} nestedHi=${m.nestedHi} isU8=${m.isU8} u8sum=${m.u8sum}`,
      );
    }
    if (replies.length === 3) {
      child.send({ cmd: "bye" });
    }
  });
  child.on("exit", () => {
    console.log("child exited; connected:", child.connected);
    resolve();
  });

  child.send({ cmd: "buf", payload: Buffer.from([1, 2, 3, 4, 5]) });

  const large = Buffer.alloc(1000);
  for (let i = 0; i < large.length; i++) large[i] = i % 256;
  child.send({ cmd: "large", payload: large });

  child.send({
    cmd: "mixed",
    payload: {
      n: 42,
      s: "hello",
      b: true,
      arr: [10, 20, 30],
      nested: { hi: "there" },
      u8: new Uint8Array([7, 8, 9]),
    },
  });
});

fs.unlinkSync(childPath);
console.log("advanced serialization done");
