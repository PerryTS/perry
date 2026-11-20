import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
process.on("warning", (warning) => {
  console.log("warning event:", warning.name, warning.message);
});
port1.on("unknown", undefined as any);
port1.once("unknown", null as any);
port1.off("unknown", null as any);
port1.removeEventListener("unknown", undefined as any);
port1.close();
port2.close();
console.log("sync done");
