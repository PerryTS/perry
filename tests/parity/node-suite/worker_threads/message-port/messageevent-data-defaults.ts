import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const pair = new MessageChannel();
const sync = new MessageChannel();
let rows: boolean[][] = [];
let seen = 0;
let finish: () => void;
function observe(index: number, value: any) {
  rows[index] = [value === undefined, value === null, value === 7];
  if (++seen === 3) finish();
}
pair.port1.on("message", (value) => observe(0, value));
pair.port1.onmessage = (event) => observe(1, event.data);
pair.port1.addEventListener("message", (event) => observe(2, event.data));

async function main() {
  for (const [label, value] of [["undefined", undefined], ["null", null], ["number", 7]] as [string, any][]) {
    rows = [];
    seen = 0;
    const done = new Promise<void>((resolve) => { finish = resolve; });
    pair.port2.postMessage(value);
    await done;
    console.log(label, JSON.stringify(rows));
    sync.port2.postMessage(value);
    const queued = receiveMessageOnPort(sync.port1)!;
    console.log("sync", label, Object.hasOwn(queued, "message"), queued.message === value);
  }
  console.log("complete", 3);
}
function close() { pair.port1.close(); pair.port2.close(); sync.port1.close(); sync.port2.close(); }
main().then(close, (error) => { close(); throw error; });
