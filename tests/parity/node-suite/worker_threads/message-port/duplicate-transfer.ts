import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

const carrier = new MessageChannel();
const buffer = new ArrayBuffer(8);
outcome(
  "duplicate buffer:",
  () => carrier.port1.postMessage(buffer, [buffer, buffer]),
);
console.log("buffer retained:", buffer.byteLength);

const movable = new MessageChannel();
outcome(
  "duplicate port:",
  () =>
    carrier.port1.postMessage(movable.port1, [movable.port1, movable.port1]),
);
movable.port1.postMessage("retained");
const retained = receiveMessageOnPort(movable.port2);
console.log("port retained:", retained ? retained.message : "missing");

carrier.port1.close();
carrier.port2.close();
movable.port1.close();
movable.port2.close();
