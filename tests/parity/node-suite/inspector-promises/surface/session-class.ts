import { Session as PromiseSession } from "node:inspector/promises";
import { Session as CallbackSession } from "node:inspector";

const session = new PromiseSession();
console.log("distinct:", PromiseSession !== CallbackSession);
console.log(
  "subclass:",
  Object.getPrototypeOf(PromiseSession) === CallbackSession,
  Object.getPrototypeOf(PromiseSession.prototype) === CallbackSession.prototype,
);
console.log(
  "instances:",
  session instanceof PromiseSession,
  session instanceof CallbackSession,
);
console.log(
  "methods:",
  PromiseSession.prototype.connect === CallbackSession.prototype.connect,
  PromiseSession.prototype.disconnect === CallbackSession.prototype.disconnect,
  PromiseSession.prototype.post === CallbackSession.prototype.post,
);
console.log(
  "own prototype:",
  Reflect.ownKeys(PromiseSession.prototype).join(","),
);
