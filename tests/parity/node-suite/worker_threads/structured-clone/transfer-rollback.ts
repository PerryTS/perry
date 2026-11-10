import { MessageChannel } from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const channel = new MessageChannel();
const buffer = new ArrayBuffer(8);
console.log(
  "invalid after buffer:",
  outcome(() => channel.port1.postMessage(buffer, [buffer, null as any])),
  buffer.byteLength,
);

const closed = new MessageChannel();
closed.port1.close();
const rollback = new ArrayBuffer(16);
console.log(
  "closed port rollback:",
  outcome(() => channel.port1.postMessage(null, [rollback, closed.port1])),
  rollback.byteLength,
);

channel.port1.close();
channel.port2.close();
closed.port2.close();
