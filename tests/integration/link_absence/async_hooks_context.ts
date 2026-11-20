// HWS companion: async_hooks
// HWS hook: js_runtime_install_async_hooks
// HWS prefixes: perry_runtime::async_hooks::
import { AsyncLocalStorage } from "node:async_hooks";
const storage = new AsyncLocalStorage();
storage.run(7, () => {
  Promise.resolve().then(() => {
    console.log(storage.getStore());
    setTimeout(() => console.log(storage.getStore()), 0);
  });
});
console.log(storage.getStore() === undefined);
