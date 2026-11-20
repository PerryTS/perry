import { Duplex } from "node:stream";

// Duplex.fromWeb wraps a { readable, writable } Web pair as one Node Duplex.
const written: string[] = [];
const readable = new ReadableStream({
  start(c) {
    c.enqueue("r");
    c.close();
  },
});
const writable = new WritableStream({ write(c) { written.push(c); } });
const pair = {
  readable,
  writable,
};

const d = (Duplex as any).fromWeb(pair, { objectMode: true });
console.log("is Duplex:", d instanceof Duplex);

const read: string[] = [];
d.on("data", (chunk) => read.push(chunk));
const finished = new Promise((resolve) => d.on("finish", resolve));
const ended = new Promise((resolve) => d.on("end", resolve));
await new Promise((resolve) => d.write("w", resolve));
d.end();

await finished;
await ended;

console.log("read:", read.join(""));
console.log("written:", written.join(""));
