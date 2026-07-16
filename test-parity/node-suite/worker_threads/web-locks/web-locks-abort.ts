import { locks } from "node:worker_threads";

async function main() {
  const controller = new AbortController();
  controller.abort(new Error("pre-aborted"));
  let callbackCalled = false;

  try {
    await locks.request("aborted-lock", { signal: controller.signal }, () => {
      callbackCalled = true;
      return "unexpected";
    });
    console.log("pre-aborted: resolved", callbackCalled);
  } catch (error: any) {
    console.log("pre-aborted:", error?.name, error?.message, callbackCalled);
  }

  const snapshot = await locks.query();
  console.log("snapshot:", snapshot.held.length, snapshot.pending.length);
}

main().catch((error) => console.log("unexpected:", error?.name, error?.message));
