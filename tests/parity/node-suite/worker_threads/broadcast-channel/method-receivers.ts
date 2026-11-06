import { BroadcastChannel } from "node:worker_threads";

function outcome(fn: () => any): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const prototype = BroadcastChannel.prototype as any;
const nameGetter = Object.getOwnPropertyDescriptor(prototype, "name")?.get;
console.log(
  "name getter:",
  typeof nameGetter,
  outcome(() => (nameGetter as any).call({})),
);

for (const name of ["postMessage", "close", "ref", "unref"] as const) {
  console.log(`${name}:`, outcome(() => prototype[name].call({})));
}
