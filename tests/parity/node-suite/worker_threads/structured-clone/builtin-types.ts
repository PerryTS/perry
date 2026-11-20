import { MessageChannel, receiveMessageOnPort } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
let postStatus = "ok";

try {
  port1.postMessage({
    date: new Date("2020-01-02T03:04:05.000Z"),
    regexp: /worker/gi,
    map: new Map([["answer", 42]]),
    set: new Set([3, 1, 4]),
    bigint: 9007199254740993n,
    error: new TypeError("clone-me"),
  });
} catch (error: any) {
  postStatus = `${error?.name}:${error?.code ?? ""}`;
}

const packet = receiveMessageOnPort(port2);
const value = packet ? packet.message : undefined;
console.log("post:", postStatus);
console.log(
  "brands:",
  value?.date instanceof Date,
  value?.regexp instanceof RegExp,
  value?.map instanceof Map,
  value?.set instanceof Set,
  typeof value?.bigint,
  value?.error instanceof TypeError,
);
console.log(
  "values:",
  value?.date?.toISOString?.(),
  value?.regexp?.source,
  value?.regexp?.flags,
  value?.map?.get?.("answer"),
  value?.set?.has?.(4),
  String(value?.bigint),
  value?.error?.message,
);

port1.close();
port2.close();
