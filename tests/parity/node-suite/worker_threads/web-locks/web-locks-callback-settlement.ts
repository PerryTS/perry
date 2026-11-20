import { locks } from "node:worker_threads";

async function main() {
  console.log("sync:", await locks.request("sync", () => 42));
  console.log(
    "promise:",
    await locks.request("promise", async () => "resolved"),
  );

  try {
    await locks.request("throw", () => {
      throw new TypeError("callback-failure");
    });
    console.log("throw: resolved");
  } catch (error: any) {
    console.log("throw:", error?.name, error?.message);
  }

  try {
    await locks.request(
      "reject",
      () => Promise.reject(new RangeError("rejected")),
    );
    console.log("reject: resolved");
  } catch (error: any) {
    console.log("reject:", error?.name, error?.message);
  }

  const snapshot = await locks.query();
  console.log("cleanup:", snapshot.held.length, snapshot.pending.length);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
