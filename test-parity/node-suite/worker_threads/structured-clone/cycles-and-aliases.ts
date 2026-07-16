import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const shared = { label: "shared" };
const source: any = { left: shared, right: shared };
source.self = source;

let postStatus = "ok";
try {
  port1.postMessage(source);
} catch (error: any) {
  postStatus = `${error?.name}:${error?.code ?? ""}`;
}

const packet = receiveMessageOnPort(port2);
const value = packet ? packet.message : undefined;
console.log("post:", postStatus);
console.log(
  "identity:",
  value?.self === value,
  value?.left === value?.right,
  value?.left === shared,
  value?.left?.label,
);

port1.close();
port2.close();
