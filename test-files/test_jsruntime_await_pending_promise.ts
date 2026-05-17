import { delayedValue } from "./fixtures/jsruntime_promise_surface/mod.js";

const value = await delayedValue("v8-pending");
console.log("awaited:", value);
