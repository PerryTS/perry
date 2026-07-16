import { BroadcastChannel } from "node:worker_threads";

const sender = new BroadcastChannel("event-metadata");
const receiver = new BroadcastChannel("event-metadata");

receiver.onmessage = (event: MessageEvent) => {
  console.log(
    "event:",
    event instanceof MessageEvent,
    event.type,
    event.data,
    event.target === receiver,
    event.currentTarget === receiver,
  );
  console.log(
    "defaults:",
    JSON.stringify({
      origin: event.origin ?? "missing",
      lastEventId: event.lastEventId ?? "missing",
      source: event.source ?? null,
      ports: event.ports?.length ?? "missing",
    }),
  );
  sender.close();
  receiver.close();
};

sender.postMessage("payload");
