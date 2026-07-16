import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
let resource!: AsyncResource;

storage.run("resource-captured", () => {
  resource = new AsyncResource("ParityStorageResource");
});

storage.enterWith("caller");
const result = resource.runInAsyncScope(
  (value: string) => {
    console.log("resource storage scope:", storage.getStore());
    return value.toUpperCase();
  },
  null,
  "value",
);

console.log("resource storage result:", result);
console.log("resource storage restored:", storage.getStore());

resource.emitDestroy();
storage.disable();
