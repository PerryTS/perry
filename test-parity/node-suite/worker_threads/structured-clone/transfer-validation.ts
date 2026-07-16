import { MessageChannel } from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const carrier = new MessageChannel();
const extra = new MessageChannel();

const duplicate = new ArrayBuffer(8);
console.log(
  "duplicate buffer:",
  outcome(() => carrier.port1.postMessage(duplicate, [duplicate, duplicate])),
  duplicate.byteLength,
);
console.log(
  "source port:",
  outcome(() => carrier.port1.postMessage(null, [carrier.port1])),
);
console.log(
  "missing port transfer:",
  outcome(() => carrier.port1.postMessage({ port: extra.port1 })),
);
console.log(
  "invalid transfer entry:",
  outcome(() => carrier.port1.postMessage("value", [1 as any])),
);

carrier.port1.close();
carrier.port2.close();
extra.port1.close();
extra.port2.close();
