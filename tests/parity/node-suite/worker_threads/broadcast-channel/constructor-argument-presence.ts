import { BroadcastChannel as Channel } from "node:worker_threads";
import * as workerThreads from "node:worker_threads";

const Captured = workerThreads.BroadcastChannel;
const constructors: [string, (args: any[]) => any][] = [
  ["import", (args) => args.length ? new Channel(args[0]) : new (Channel as any)()],
  ["namespace", (args) => args.length ? new workerThreads.BroadcastChannel(args[0]) : new (workerThreads.BroadcastChannel as any)()],
  ["captured", (args) => Reflect.construct(Captured, args)],
  ["global", (args) => args.length ? new globalThis.BroadcastChannel(args[0]) : new (globalThis.BroadcastChannel as any)()],
  ["reflect", (args) => Reflect.construct(Channel, args)],
  ["spread", (args) => new (Channel as any)(...args)],
];
for (const [label, construct] of constructors) {
  for (const [kind, args] of [["missing", []], ["undefined", [undefined]], ["symbol", [Symbol("name")]]] as [string, any[]][]) {
    try {
      const channel = construct(args);
      console.log(label, kind, channel.name, typeof channel.name);
      channel.close();
    } catch (error: any) {
      console.log(label, kind, error.name, error.code ?? "");
    }
  }
}

let conversions = 0;
const hints: string[] = [];
const named = new Channel({
  [Symbol.toPrimitive](hint: string) {
    conversions++;
    hints.push(hint);
    return "normalized";
  },
} as any);
console.log("conversion", named.name, conversions, hints.join(","));
named.close();

const thrown = { marker: "identity" };
for (const [label, value] of [
  ["exotic-symbol", { [Symbol.toPrimitive]: () => Symbol("name") }],
  ["ordinary-symbol", { toString: () => Symbol("name") }],
  ["throw", { toString() { throw thrown; } }],
] as const) {
  try {
    const channel = new Channel(value as any);
    console.log(label, "accepted");
    channel.close();
  } catch (error: any) {
    console.log(label, error === thrown, error.name ?? "");
  }
}
let extras = 0;
const extra = new (Channel as any)("extra", ++extras);
console.log("extra", extra.name, extras);
extra.close();
