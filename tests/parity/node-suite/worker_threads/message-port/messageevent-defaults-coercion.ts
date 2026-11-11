import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();

for (
  const [label, event] of [
    ["default", new MessageEvent("message")],
    [
      "undefined",
      new MessageEvent("message", { data: undefined, origin: "origin" }),
    ],
    [
      "coerced",
      new MessageEvent("message", {
        data: 2,
        origin: 1 as any,
        lastEventId: 0 as any,
      }),
    ],
    [
      "source",
      new MessageEvent("messageerror", {
        lastEventId: "event",
        source: port1,
      }),
    ],
  ] as const
) {
  console.log(
    label,
    event.type,
    event.data,
    event.origin,
    event.lastEventId,
    event.source === port1 ? "port" : event.source,
    event.ports?.length ?? "missing",
  );
}

for (
  const [label, init] of [
    ["number source", { source: 1 }],
    ["noniterable ports", { ports: 0 }],
    ["null port", { ports: [null] }],
  ] as const
) {
  try {
    new MessageEvent("message", init as any);
    console.log(label, "ok");
  } catch (error) {
    console.log(label, (error as any)?.name, (error as any)?.code ?? "");
  }
}

port1.close();
port2.close();
