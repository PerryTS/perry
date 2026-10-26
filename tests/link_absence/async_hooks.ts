// HWS hook: none (no hook yet)
// HWS prefixes: perry_runtime::async_hooks::
import { AsyncLocalStorage } from "node:async_hooks"; const s=new AsyncLocalStorage(); s.run(7,()=>console.log(s.getStore()));
