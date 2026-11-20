import * as dc from "node:diagnostics_channel";

function result(label: string, fn: () => unknown) {
  try {
    fn();
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code");
  }
}

result("number tracingChannel", () => dc.tracingChannel(0 as any));
result("bad channel object", () => dc.tracingChannel({ start: "not-channel" } as any));
result("empty channel object", () => dc.tracingChannel({} as any));
