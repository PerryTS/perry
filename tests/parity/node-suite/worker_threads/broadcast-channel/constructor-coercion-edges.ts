import { BroadcastChannel } from "node:worker_threads";

for (
  const [label, value] of [
    ["null", null],
    ["bigint", 1n],
    ["boolean", false],
    ["infinity", Number.POSITIVE_INFINITY],
    ["symbol", Symbol("channel")],
  ] as const
) {
  try {
    const channel = new BroadcastChannel(value as any);
    console.log(label, "name", channel.name);
    channel.close();
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}
