import { BroadcastChannel } from "node:worker_threads";

for (
  const [label, value] of [
    ["missing", undefined],
    ["empty", ""],
    ["number", 42],
    ["object", { toString: () => "object-name" }],
  ] as const
) {
  try {
    const channel = value === undefined
      ? new (BroadcastChannel as any)()
      : new BroadcastChannel(value as any);
    console.log(label, "name", channel.name, typeof channel.name);
    channel.close();
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}
