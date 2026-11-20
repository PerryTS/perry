// HWS companion: async_hooks
// HWS hook: js_runtime_install_async_hooks
// HWS prefixes: perry_runtime::async_hooks::
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const hooks = () => require("node:async_hooks");
setTimeout(() => console.log(hooks().executionAsyncId() > 1), 0);
