import * as dc from "node:diagnostics_channel";

function result(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code");
  }
}

result("null channel", () => dc.channel(null as any));
result("number channel", () => dc.channel(1 as any));
const ch = dc.channel("dc-validation");
result("null subscribe", () => ch.subscribe(null as any));
result("module null subscribe", () => dc.subscribe("dc-validation", null as any));
