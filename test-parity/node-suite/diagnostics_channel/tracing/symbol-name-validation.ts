import { channel, tracingChannel } from "node:diagnostics_channel";

function result(label: string, fn: () => unknown) {
  try {
    console.log(`${label}:`, fn());
  } catch (err: any) {
    console.log(`${label}:`, err?.name, err?.code || "no-code");
  }
}

result("plain channel symbol", () => typeof channel(Symbol("plain")));
result("trace local symbol", () => typeof tracingChannel(Symbol("s") as any));
result("trace registry symbol", () => typeof tracingChannel(Symbol.for("shared") as any));
