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

const url = new URL("https://example.org/path?q=value");
const channel = new MessageChannel();
console.log("port:", outcome(() => channel.port1.postMessage(url)));
console.log("port queue:", receiveMessageOnPort(channel.port2));
channel.port1.close();
channel.port2.close();

const broadcast = new BroadcastChannel("url-rejection");
console.log("broadcast:", outcome(() => broadcast.postMessage(url)));
broadcast.close();
