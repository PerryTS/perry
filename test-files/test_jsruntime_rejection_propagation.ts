import { rejectLater } from "./fixtures/jsruntime_promise_surface/mod.js";

try {
  await rejectLater("v8-reject");
  console.log("missing rejection");
} catch (err: any) {
  console.log("caught:", err);
}
