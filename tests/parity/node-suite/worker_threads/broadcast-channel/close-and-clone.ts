import {
  BroadcastChannel,
  MessageChannel,
  receiveMessageOnPort,
} from "node:worker_threads";

function outcome(fn: () => void): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const sender = new BroadcastChannel("clone");
const receiver = new BroadcastChannel("clone");
const view = new Uint8Array([3, 1, 4]);
sender.postMessage(view);
view[0] = 9;

const packet = receiveMessageOnPort(receiver);
const cloned = packet ? packet.message : undefined;
const clonedValues = typeof cloned?.join === "function"
  ? cloned.join(",")
  : "not-typed";
console.log(
  "typed clone:",
  cloned instanceof Uint8Array,
  clonedValues,
  view.join(","),
);

const channel = new MessageChannel();
console.log(
  "port rejected:",
  outcome(() => sender.postMessage({ port: channel.port1 })),
);

sender.close();
console.log("post closed:", outcome(() => sender.postMessage("late")));

receiver.close();
channel.port1.close();
channel.port2.close();
