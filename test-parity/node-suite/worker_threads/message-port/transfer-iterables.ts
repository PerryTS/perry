import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "", error?.message);
  }
}

const { port1, port2 } = new MessageChannel();
const valid = new ArrayBuffer(4);
new Uint8Array(valid)[0] = 7;
port1.postMessage(
  { valid },
  { transfer: new Set([valid]) } as any,
);
const packet = receiveMessageOnPort(port2);
const received = packet ? packet.message?.valid : undefined;
console.log(
  "valid set:",
  valid.byteLength,
  received instanceof ArrayBuffer,
  received instanceof ArrayBuffer ? new Uint8Array(received)[0] : "missing",
);

let iterations = 0;
const throwing = {
  *[Symbol.iterator]() {
    iterations += 1;
    throw new Error("iterator-boom");
  },
};
outcome(
  "throwing iterable:",
  () => port1.postMessage({}, { transfer: throwing } as any),
);
console.log("iterations:", iterations);

outcome(
  "invalid set:",
  () => port1.postMessage({}, { transfer: new Set([5]) } as any),
);
outcome("noniterable:", () => port1.postMessage({}, { transfer: 5 } as any));

port1.close();
port2.close();
