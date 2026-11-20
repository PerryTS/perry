import { AsyncLocalStorage } from "node:async_hooks";
import { lookupService } from "node:dns";

const storage = new AsyncLocalStorage<string>();
let callbackInvoked = false;
let resultShapeValid = false;

await storage.run(
  "lookup-service",
  () =>
    new Promise<void>((resolve) => {
      lookupService("127.0.0.1", 80, (error, hostname, service) => {
        callbackInvoked = true;
        resultShapeValid =
          error !== null ||
          (typeof hostname === "string" && typeof service === "string");
        console.log("lookupService store:", storage.getStore());
        resolve();
      });
    }),
);

console.log(
  "lookupService callback/result:",
  callbackInvoked,
  resultShapeValid,
);
console.log("lookupService outside:", String(storage.getStore()));
