// HWS companion: async_hooks
// HWS hook: js_runtime_install_async_hooks
// HWS prefixes: perry_runtime::async_hooks::
import { AsyncResource, createHook, executionAsyncId } from "node:async_hooks";
const seen: string[] = []; let tracked = -1;
const hook = createHook({
  init(id: number, type: string) { if (type === "HWS2") { tracked = id; seen.push("init"); } },
  before(id: number) { if (id === tracked) seen.push("before"); },
  after(id: number) { if (id === tracked) seen.push("after"); }
});
hook.enable();
const resource = new AsyncResource("HWS2");
resource.runInAsyncScope(() => console.log(executionAsyncId() === resource.asyncId()));
try { resource.runInAsyncScope(() => { throw new Error("scope"); }); }
catch (error: any) { console.log(error.message); }
hook.disable(); resource.emitDestroy();
console.log(seen.join(","));
