import { AsyncResource } from "node:async_hooks";

const resource = new AsyncResource("ParityScopeReceivers");
function receiver(this: unknown) {
  if (this === undefined) return "undefined";
  if (this === null) return "null";
  if (this === false) return "false";
  if (this === "value") return "value";
  if (this && typeof this === "object" && (this as any).tag === "object") {
    return "object";
  }
  return "other";
}
console.log("scope omitted receiver:", resource.runInAsyncScope(receiver));
console.log("scope null receiver:", resource.runInAsyncScope(receiver, null));
console.log("scope false receiver:", resource.runInAsyncScope(receiver, false));
console.log(
  "scope string receiver:",
  resource.runInAsyncScope(receiver, "value"),
);
console.log(
  "scope object receiver:",
  resource.runInAsyncScope(receiver, { tag: "object" }),
);
resource.emitDestroy();
