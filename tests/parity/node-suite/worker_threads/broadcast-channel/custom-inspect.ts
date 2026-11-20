import { BroadcastChannel } from "node:worker_threads";
import { inspect } from "node:util";

function outcome(fn: () => unknown): string {
  try {
    return String(fn());
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const channel = new BroadcastChannel("inspect-channel");
const custom = (channel as any)[inspect.custom];
console.log("custom:", typeof custom, outcome(() => custom.call({})));
console.log("active:", inspect(channel), inspect(channel, { depth: -1 }));
channel.close();
console.log("closed:", inspect(channel));
