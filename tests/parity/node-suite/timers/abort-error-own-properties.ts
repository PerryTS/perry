// The AbortError a pre-aborted signal rejects with has own `code` then `name`.
import { setTimeout as sleep } from "node:timers/promises";

const controller = new AbortController();
controller.abort();
try {
  await sleep(1, null, { signal: controller.signal });
} catch (error: any) {
  console.log(error.name, error.code, Object.keys(error).join(","), JSON.stringify(error));
  console.log(error instanceof Error, error.message);
}
