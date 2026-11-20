import { MessageChannel, MessagePort } from "node:worker_threads";

function outcome(fn: () => unknown): string {
  try {
    fn();
    return "created";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const channel = new MessageChannel();
console.log(
  "brand:",
  channel.port1 instanceof MessagePort,
  channel.port1.constructor === MessagePort,
);
console.log("port call:", outcome(() => (MessagePort as any)()));
console.log("port new:", outcome(() => new (MessagePort as any)()));
console.log("channel call:", outcome(() => (MessageChannel as any)()));
channel.port1.close();
channel.port2.close();
