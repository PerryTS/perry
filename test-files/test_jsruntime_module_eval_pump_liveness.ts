import { touchModuleEvalPump } from "./fixtures/jsruntime_promise_surface/module_eval_pump.js";

const loaded = touchModuleEvalPump();
if (loaded !== "loaded") {
  console.log("module-pump: missing");
}
