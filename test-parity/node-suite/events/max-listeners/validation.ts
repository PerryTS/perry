import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";

function showEmitter(label: string, value: any): void {
  const emitter = new EventEmitter();
  try {
    const result = emitter.setMaxListeners(value);
    console.log(label, "OK", String(emitter.getMaxListeners()), result === emitter);
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

function showStream(label: string, value: any): void {
  const stream = new PassThrough();
  try {
    const result = stream.setMaxListeners(value);
    console.log(label, "OK", String(stream.getMaxListeners()), result === stream);
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

for (const value of [0, 1, Infinity, -1, NaN, "5", null, undefined, 1.5]) {
  showEmitter(`emitter ${String(value)}`, value);
}

showStream("stream infinity", Infinity);
showStream("stream negative", -1);
showStream("stream string", "5");
showStream("stream nan", NaN);
