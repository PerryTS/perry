import { tracingChannel } from "node:diagnostics_channel";

const ch = tracingChannel("dc-subscribe-non-callable");
// Each handler key must be a function; passing a non-callable should
// throw ERR_INVALID_ARG_TYPE. The previous impl silently skipped the
// bad handler, which masked the bug.
function check(label: string, handlers: any): void {
  try {
    ch.subscribe(handlers);
    ch.unsubscribe(handlers);
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code || "no-code");
  }
}

check("start=42", { start: 42 });
check("end=null", { end: null });
check("asyncEnd=string", { asyncEnd: "x" });
