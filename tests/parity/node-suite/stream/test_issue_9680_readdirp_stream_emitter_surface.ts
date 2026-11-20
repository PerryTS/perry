// Regression for #9680: the bundled readdirp source aliases its node:stream
// import, subclasses it, and hands the instance to chokidar. Chokidar chains
// on(...).on(...).once(...), so the stream must retain the complete inherited
// EventEmitter surface even though the native base's local name was minified.

import { EventEmitter as jt } from "node:events";
import { Readable as ut } from "node:stream";

class ReaddirpStyleStream extends ut {
  readCalls = 0;

  constructor() {
    super({ objectMode: true, autoDestroy: true, highWaterMark: 4096 });
  }

  _read(): void {
    this.readCalls++;
  }
}

const Watcher = class extends jt {
  marker(): string {
    return "watcher-subclass";
  }
};

function readdirpStyle(): any {
  return new ReaddirpStyleStream();
}

const stream = readdirpStyle();
const emitterMethods = [
  "on",
  "once",
  "off",
  "removeListener",
  "emit",
  "prependListener",
  "prependOnceListener",
  "listenerCount",
  "eventNames",
];

const expectedNames = new Set(emitterMethods);
const surfaceNames = new Set<string>();
let prototypeCursor: any = stream;
while (prototypeCursor !== null) {
  for (const name of Object.getOwnPropertyNames(prototypeCursor)) {
    if (expectedNames.has(name)) surfaceNames.add(name);
  }
  prototypeCursor = Object.getPrototypeOf(prototypeCursor);
}

stream._read();
console.log("read-calls:" + stream.readCalls);
console.log("surface:" + Array.from(surfaceNames).sort().join(","));
console.log(emitterMethods.map((name) => `${name}:${typeof stream[name]}`).join(","));

const calls: string[] = [];
const watcher: any = new Watcher();
watcher.once("ready", (): void => calls.push("watcher-ready"));
watcher.emit("ready");
console.log("watcher:" + watcher.marker());

const persistent = (value: string): void => calls.push(`on:${value}`);
stream
  .on("entry", persistent)
  .on("unused", (): void => calls.push("unused"))
  .once("entry", (value: string): void => calls.push(`once:${value}`));
stream.prependListener("entry", (value: string): void => calls.push(`prepend:${value}`));
stream.prependOnceListener("entry", (value: string): void => calls.push(`prepend-once:${value}`));

console.log("events-before:" + stream.eventNames().join(","));
console.log("listeners-before:" + stream.listenerCount("entry"));
console.log("emit-1:" + stream.emit("entry", "a"));
console.log("emit-2:" + stream.emit("entry", "b"));
stream.off("entry", persistent);
console.log("listeners-after:" + stream.listenerCount("entry"));
console.log("calls:" + calls.join(","));
