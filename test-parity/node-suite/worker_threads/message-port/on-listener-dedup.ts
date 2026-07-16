import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const sharedCalls: string[] = [];

function shared(value: unknown) {
  sharedCalls.push(String(value));
}

port1.on("message", shared);
port1.on("message", shared);
port1.on("message", shared);

port1.on("message", (value) => {
  if (value === "first") {
    console.log("first:", sharedCalls.join(","));
    port1.off("message", shared);
    port2.postMessage("after-off");
  } else if (value === "after-off") {
    console.log("after off:", sharedCalls.join(","));
    port1.close();
    port2.close();
  }
});

port2.postMessage("first");
