import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const event = new MessageEvent("message", {
  data: { value: 1 },
  origin: "worker-origin",
  lastEventId: "event-1",
  source: port1,
  ports: [port2],
});

console.log(
  "event:",
  event.type,
  event.data?.value ?? "missing",
  event.origin,
  event.lastEventId,
);
console.log("source:", event.source === port1);
console.log(
  "ports:",
  event.ports?.length ?? "missing",
  event.ports?.[0] === port2,
);
console.log("brands:", event instanceof Event, event instanceof MessageEvent);

for (
  const [label, init] of [
    ["bad source", { source: {} }],
    ["bad ports", { ports: [{}] }],
  ] as const
) {
  try {
    new MessageEvent("message", init as any);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

port1.close();
port2.close();
