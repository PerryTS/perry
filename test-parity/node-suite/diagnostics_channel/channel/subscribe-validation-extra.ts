import { channel, hasSubscribers } from "node:diagnostics_channel";

function show(label: string, fn: () => unknown): void {
  try { console.log(label + ":", fn()); } catch (err: any) { console.log(label + ":", err?.name, err?.code || "no-code"); }
}
show("has numeric", () => hasSubscribers(123 as any));
show("channel symbol", () => channel(Symbol.for("dc-symbol-extra") as any).name.toString());
const ch = channel("dc-validation-extra");
show("subscribe null", () => ch.subscribe(null as any));
show("unsubscribe null", () => ch.unsubscribe(null as any));
