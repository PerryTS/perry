import { scheduler } from "node:timers/promises";

try {
  (scheduler as any)();
} catch (err: any) {
  console.log("scheduler callable:", err instanceof TypeError, err.name);
}
