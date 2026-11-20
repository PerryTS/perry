import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const buffer = new ArrayBuffer(16);
const bytes = new Uint8Array(buffer);
const dataView = new DataView(buffer);
dataView.setFloat64(0, Math.PI);
bytes[8] = 99;

port1.postMessage({ dataView, bytes }, [buffer]);
let dataViewLength: number | string;
try {
  dataViewLength = dataView.byteLength;
} catch (error: any) {
  dataViewLength = error?.name;
}
console.log("source detached:", buffer.byteLength, bytes.byteLength, dataViewLength);

const packet = receiveMessageOnPort(port2);
const value = packet ? packet.message : undefined;
console.log(
  "brands/backing:",
  value?.dataView instanceof DataView,
  value?.bytes instanceof Uint8Array,
  value?.dataView?.buffer === value?.bytes?.buffer,
);
console.log(
  "values:",
  value?.dataView?.getFloat64?.(0),
  value?.bytes?.[8],
);

port1.close();
port2.close();
