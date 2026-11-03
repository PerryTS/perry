// The events module, bundled into the runtime: ordering, once, error events,
// listener bookkeeping and async iteration over events.
import { EventEmitter, once } from "node:events";

const bus = new EventEmitter();
const seen: string[] = [];
bus.on("tick", (n: number) => seen.push(`a${n}`));
bus.prependListener("tick", (n: number) => seen.push(`first${n}`));
bus.once("tick", (n: number) => seen.push(`once${n}`));
bus.emit("tick", 1);
bus.emit("tick", 2);
console.log(seen.join(","), bus.listenerCount("tick"), bus.eventNames().join(","));

try {
  bus.emit("error", new Error("unhandled"));
} catch (e: any) {
  console.log("thrown", e.message);
}

class Job extends EventEmitter {
  run() {
    setTimeout(() => this.emit("done", "finished"), 1);
  }
}

async function main() {
  const job = new Job();
  job.run();
  const [value] = await once(job, "done");
  console.log(value);
  bus.removeAllListeners("tick");
  console.log(bus.listenerCount("tick"));
}
main();
