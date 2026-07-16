import { MessageChannel } from "node:worker_threads";

for (
  const [label, args] of [
    ["none", []],
    ["one", [1]],
    ["many", [1, 2, 3]],
  ] as const
) {
  try {
    const channel = Reflect.construct(MessageChannel, args as readonly any[]);
    console.log(
      label,
      channel.port1 instanceof MessagePort,
      channel.port2 instanceof MessagePort,
    );
    channel.port1.close();
    channel.port2.close();
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}
