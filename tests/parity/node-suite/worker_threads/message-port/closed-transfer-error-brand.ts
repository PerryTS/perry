import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const { port1: target1, port2: target2 } = new MessageChannel();
const buffer = new ArrayBuffer(8);

port2.close();

try {
  target1.postMessage({ buffer, port: port2 }, [buffer, port2]);
  console.log("transfer: ok");
} catch (error: any) {
  console.log(
    "error:",
    error?.name,
    error?.code ?? "",
    error instanceof Error,
    error instanceof DOMException,
    error?.constructor?.name,
  );
}
console.log("rollback:", buffer.byteLength, port2 instanceof MessagePort);

port1.close();
port2.close();
target1.close();
target2.close();
