import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

function removed(value: any) {
  events.push(`removed:${value}`);
}

port1.on("message", removed);
port1.off("message", removed);
port1.once("message", (value) => events.push(`once:${value}`));
port1.on("message", (value) => {
  events.push(`on:${value}`);
  if (value === "second") {
    port1.close();
  }
});
port1.on("close", () => {
  console.log("events:", events.join(","));
  const count = typeof (port1 as any).listenerCount === "function"
    ? `${(port1 as any).listenerCount("message")},${
      (port1 as any).listenerCount("close")
    }`
    : "unsupported";
  console.log("counts:", count);
  port2.close();
});

port2.postMessage("first");
port2.postMessage("second");
