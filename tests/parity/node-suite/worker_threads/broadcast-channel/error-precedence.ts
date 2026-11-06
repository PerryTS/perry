import { BroadcastChannel } from "node:worker_threads";

function outcome(channel: BroadcastChannel, value: any): string {
  try {
    channel.postMessage(value);
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const channel = new BroadcastChannel("error-precedence");
console.log("open symbol:", outcome(channel, Symbol("value")));
channel.close();
console.log("closed symbol:", outcome(channel, Symbol("value")));
