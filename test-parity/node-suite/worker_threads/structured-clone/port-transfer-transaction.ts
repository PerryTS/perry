import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const carrier = new MessageChannel();
const movable = new MessageChannel();
const buffer = new ArrayBuffer(8);
let calls = 0;
const failure = { sentinel: true };
const throwing = { get value() { calls++; throw failure; } };
try {
  carrier.port1.postMessage(throwing, [movable.port1, buffer]);
} catch (error) {
  console.log("getter identity", error === failure, calls, buffer.byteLength,
    receiveMessageOnPort(carrier.port2));
}
movable.port1.postMessage("retained");
console.log("ownership", receiveMessageOnPort(movable.port2)?.message);
try {
  carrier.port1.postMessage({ port: movable.port1, unsupported() {} }, [movable.port1, buffer]);
} catch (error: any) {
  console.log("unsupported", error.name, error.code, buffer.byteLength,
    receiveMessageOnPort(carrier.port2));
}
const order: number[] = [];
const entries = [movable.port1, movable.port1];
Object.defineProperty(entries, 0, { get() { order.push(0); return movable.port1; } });
Object.defineProperty(entries, 1, { get() { order.push(1); return movable.port1; } });
try {
  carrier.port1.postMessage({ get graph() { order.push(2); return true; } }, entries);
} catch (error: any) {
  console.log("admission", order.join(","), error.name, error.code, error.message);
}
const inner = new MessageChannel();
const reentrant = { get move() {
  inner.port1.postMessage({ port: movable.port1, alias: movable.port1 }, [movable.port1]);
  return true;
}, port: movable.port1 };
try {
  carrier.port1.postMessage(reentrant, [movable.port1, buffer]);
} catch (error: any) {
  console.log("reentrant", error.name, error.code, error.message, buffer.byteLength,
    receiveMessageOnPort(carrier.port2));
}
const adopted = receiveMessageOnPort(inner.port2)!.message;
console.log("inner alias", adopted.port === adopted.alias);
adopted.port.postMessage("inner-owner");
console.log("inner committed", receiveMessageOnPort(movable.port2)?.message);
adopted.port.close();
movable.port1.close();
movable.port2.close();
carrier.port1.close();
carrier.port2.close();
inner.port1.close();
inner.port2.close();
