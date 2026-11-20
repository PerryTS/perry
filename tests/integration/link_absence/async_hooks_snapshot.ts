// HWS companion: async_hooks
// HWS hook: js_runtime_install_async_hooks
// HWS prefixes: perry_runtime::async_hooks::
import { AsyncLocalStorage } from "node:async_hooks";
const storage = new AsyncLocalStorage();
const snapshot = storage.run(11, () => AsyncLocalStorage.snapshot());
console.log(storage.run(22, () => snapshot(() => storage.getStore())));
console.log(storage.getStore() === undefined);
const bound = storage.run(13, () => AsyncLocalStorage.bind(() => storage.getStore()));
console.log(bound());
try { snapshot(() => { throw new Error("snapshot"); }); }
catch (error: any) { console.log(error.message, storage.getStore() === undefined); }
