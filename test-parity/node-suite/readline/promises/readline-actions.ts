import { Readline } from "node:readline/promises";
import { Writable } from "node:stream";

const writes: string[] = [];
const output = new Writable({
  write(chunk, _encoding, callback) {
    writes.push(String(chunk));
    callback();
  },
});

const rl = new Readline(output, { autoCommit: false });
const chain = [
  rl.clearLine(0) === rl,
  rl.cursorTo(3, 4) === rl,
  rl.moveCursor(2, -1) === rl,
  rl.clearScreenDown() === rl,
];

console.log("batch before:", chain.join("|"), JSON.stringify(writes.join("")));
console.log("surface:", typeof rl.commit, typeof rl.rollback);
console.log("commit result:", await rl.commit());
console.log("batch after:", JSON.stringify(writes.join("")));

writes.length = 0;
console.log("rollback chain:", rl.cursorTo(9, 1) === rl, rl.rollback() === rl);
await rl.commit();
console.log("rollback after:", JSON.stringify(writes.join("")));

const autoWrites: string[] = [];
const autoOutput = new Writable({
  write(chunk, _encoding, callback) {
    autoWrites.push(String(chunk));
    callback();
  },
});
const auto = new Readline(autoOutput, { autoCommit: true });

console.log("auto chain:", auto.clearLine(1) === auto, JSON.stringify(autoWrites.join("")));
await new Promise<void>((resolve) => setImmediate(resolve));
console.log("auto later:", JSON.stringify(autoWrites.join("")));
