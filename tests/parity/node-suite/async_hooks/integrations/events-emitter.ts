import { EventEmitter, EventEmitterAsyncResource } from "node:events";
import { AsyncLocalStorage, executionAsyncId } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "events",
  () =>
    new Promise<void>((resolve) => {
      const emitter = new EventEmitter();
      emitter.on("sync", (value) => {
        console.log("event sync store:", storage.getStore(), value);
      });
      emitter.once("once", () => {
        console.log("event once store:", storage.getStore());
      });
      emitter.on("async", async () => {
        await Promise.resolve();
        console.log("event async store:", storage.getStore());
        resolve();
      });

      process.nextTick(() => {
        emitter.emit("sync", "value");
        emitter.emit("once");
        emitter.emit("once");
        emitter.emit("async");
      });
    }),
);

console.log("events outside:", String(storage.getStore()));

let eventNameConversions = 0;
const convertedName = {
  toString() {
    eventNameConversions += 1;
    return "converted";
  },
};
const conversionEmitter = new EventEmitter();
let convertedValue = "missing";
conversionEmitter.on("converted", (value) => {
  convertedValue = value;
});
conversionEmitter.emit(convertedName as unknown as string, "value");
console.log("event name conversion:", eventNameConversions, convertedValue);

const scopedConversionEmitter = storage.run(
  "conversion-resource",
  () => new EventEmitterAsyncResource({ name: "ConversionEmitter" }),
);
let scopedConversions = 0;
let conversionAsyncIdsMatch = true;
const conversionStores: Array<string | undefined> = [];
const scopedName = {
  toString() {
    scopedConversions += 1;
    conversionAsyncIdsMatch &&=
      executionAsyncId() === scopedConversionEmitter.asyncId;
    conversionStores.push(storage.getStore());
    return "converted";
  },
};
scopedConversionEmitter.on("converted", () => {});
storage.run("conversion-caller", () => {
  scopedConversionEmitter.emit(scopedName as unknown as string);
  scopedConversionEmitter.emit(scopedName as unknown as string, "value");
});
console.log("scoped event name conversion:", scopedConversions);
console.log("scoped event name async id:", conversionAsyncIdsMatch);
console.log("scoped event name store:", conversionStores.join(","));
scopedConversionEmitter.emitDestroy();
