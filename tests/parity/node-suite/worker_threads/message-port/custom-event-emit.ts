import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const events: string[] = [];

port2.addEventListener("custom", (event: any) => {
  events.push(
    `event:${event.type}:${event.detail}:${event.target === port2}`,
  );
});
port2.on("custom", (detail) => events.push(`node:${detail}`));

const emit = (port2 as any).emit;
console.log(
  "emit return:",
  typeof emit === "function"
    ? emit.call(port2, "custom", "value")
    : "unsupported",
);
console.log("events:", events.join(","));

port1.close();
port2.close();
