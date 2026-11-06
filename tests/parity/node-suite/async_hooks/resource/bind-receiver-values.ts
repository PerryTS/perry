import { AsyncResource } from "node:async_hooks";

const resource = new AsyncResource("ParityBindReceivers");
function receiver(this: unknown) {
  if (this === undefined) return "undefined";
  if (this === false) return "false";
  if (this === "value") return "value";
  if (this && typeof this === "object" && (this as any).tag === "object") {
    return "object";
  }
  return "other";
}
const objectReceiver = { tag: "object" };
console.log("bound undefined receiver:", resource.bind(receiver)());
console.log("bound false receiver:", resource.bind(receiver, false)());
console.log("bound string receiver:", resource.bind(receiver, "value")());
console.log(
  "bound object receiver:",
  resource.bind(receiver, objectReceiver)(),
);
console.log(
  "bound explicit receiver ignores call:",
  resource.bind(receiver, false).call(objectReceiver),
);
console.log(
  "bound implicit receiver uses call:",
  resource.bind(receiver).call(objectReceiver),
);
resource.emitDestroy();
