import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("data-defaults");
const receiver = new BroadcastChannel("data-defaults");
let rows: boolean[][] = [];
let seen = 0;
let finish: () => void;
function observe(index: number, value: any) {
  rows[index] = [value === undefined, value === null, value === 7];
  if (++seen === 2) finish();
}
receiver.onmessage = (event) => observe(0, event.data);
receiver.addEventListener("message", (event) => observe(1, event.data));

async function main() {
  for (const [label, value] of [["undefined", undefined], ["null", null], ["number", 7]] as [string, any][]) {
    rows = [];
    seen = 0;
    const done = new Promise<void>((resolve) => { finish = resolve; });
    sender.postMessage(value);
    await done;
    console.log(label, JSON.stringify(rows));
  }
  console.log("complete", 3);
}
main().then(() => { sender.close(); receiver.close(); }, (error) => {
  sender.close(); receiver.close(); throw error;
});
