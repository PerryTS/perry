import { createHash, createHmac } from "node:crypto";
import { EventEmitter } from "node:events";
import { Readable } from "node:stream";

const tick = () => new Promise<void>((resolve) => setImmediate(resolve));

async function digestProtocol(label: string, make: () => any) {
  try {
    const digest = make().setEncoding("hex");
    const seen: string[] = [];
    const a = () => seen.push("A");
    const b = () => seen.push("B");
    digest.on("data", a).on("data", b).on("data", a);
    console.log(label, "remove returns receiver:", digest.removeListener("data", a) === digest);
    console.log(label, "counts:", digest.listenerCount("data"), digest.listenerCount("data", a), digest.listenerCount("data", b), digest.listenerCount("missing"));
    console.log(label, "nullish filter:", digest.listenerCount("data", null), digest.listenerCount("data", undefined));
    console.log(label, "absent remove:", digest.removeListener("missing", a) === digest);
    try {
      digest.removeListener("missing", 0);
    } catch (error: any) {
      console.log(label, "invalid listener:", error.code);
    }
    const done = new Promise<void>((resolve) => digest.on("end", resolve));
    digest.end("abc");
    await done;
    await tick();
    console.log(label, "survivor order:", seen.join(""));
    console.log(label, "closed remove:", digest.removeListener("data", a) === digest, digest.listenerCount("data"));

    const piped = make().setEncoding("hex");
    const output: string[] = [];
    const expected = make().update("abcdef").digest("hex");
    piped.on("data", (chunk: any) => output.push(String(chunk)));
    const pipedDone = new Promise<void>((resolve, reject) => {
      piped.on("end", resolve);
      piped.on("error", reject);
    });
    Readable.from(["abc", "def"]).pipe(piped);
    await pipedDone;
    await tick();
    console.log(label, "pipe digest:", output.join("") === expected);
    console.log(label, "cleanup counts:", piped.listenerCount("error"), piped.listenerCount("finish"), piped.listenerCount("close"), piped.listenerCount("unpipe"));
  } catch (error: any) {
    console.log(label, "protocol error:", error.code || error.name);
  }
}

async function foreignProtocol(label: string, manualEnd: boolean) {
  try {
    const destination: any = new EventEmitter();
    const prototype = Object.create(Object.getPrototypeOf(destination));
    const trace: string[] = [];
    let writeGets = 0;
    let endGets = 0;
    let receiverMatches = true;
    let writes = 0;
    let finishes = 0;
    let paused = false;
    let waitedForDrain = false;
    const source = new Readable({ read() {} });
    source.on("pause", () => { paused = true; });
    function write(this: any, chunk: any) {
      receiverMatches = receiverMatches && this === destination;
      writes++;
      trace.push("write:" + String(chunk));
      if (writes === 1) {
        setImmediate(() => {
          waitedForDrain = writes === 1;
          trace.push("drain");
          destination.emit("drain");
        });
        return false;
      }
      return true;
    }
    function end(this: any) {
      receiverMatches = receiverMatches && this === destination;
      trace.push("end");
      destination.emit("finish");
    }
    Object.defineProperty(prototype, "write", {
      configurable: true,
      get() { writeGets++; return write; },
    });
    Object.defineProperty(prototype, "end", {
      configurable: true,
      get() { endGets++; return end; },
    });
    Object.setPrototypeOf(destination, prototype);
    if (manualEnd) Object.defineProperty(destination, "write", { value: write, configurable: true });
    const done = new Promise<void>((resolve) => {
      destination.on("finish", () => { finishes++; trace.push("finish"); resolve(); });
    });
    if (manualEnd) source.on("end", () => setImmediate(() => destination.end()));
    console.log(label, "pipe returns receiver:", source.pipe(destination, { end: !manualEnd }) === destination);
    source.push("x");
    source.push("y");
    source.push(null);
    await done;
    await tick();
    console.log(label, "trace:", trace.join(","));
    console.log(label, "lookup and receiver:", writeGets, endGets, receiverMatches);
    console.log(label, "backpressure:", paused, waitedForDrain);
    console.log(label, "finish and cleanup:", finishes, destination.listenerCount("drain"), destination.listenerCount("error"), destination.listenerCount("unpipe"));
  } catch (error: any) {
    console.log(label, "protocol error:", error.code || error.name);
  }
}

await digestProtocol("hash", () => createHash("sha256"));
await digestProtocol("hmac", () => createHmac("sha256", "key"));
await foreignProtocol("inherited", false);
await foreignProtocol("own end:false", true);
