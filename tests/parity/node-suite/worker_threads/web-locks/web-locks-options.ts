import { locks } from "node:worker_threads";

async function errorShape(label: string, fn: () => any) {
  try {
    const value = fn();
    if (value && typeof value.then === "function") {
      await value;
    }
    console.log(`${label}: ok`);
  } catch (err: any) {
    console.log(`${label}:`, err?.constructor?.name, err?.name, err?.code);
  }
}

async function main() {
  await errorShape("missing callback", () => locks.request("missing-callback" as any));
  await errorShape("bad callback", () => locks.request("bad-callback", 1 as any));
  await errorShape("bad options", () => locks.request("bad-options", 1 as any, () => "ok"));
  await errorShape("string options", () => locks.request("string-options", "bad" as any, () => "ok"));
  await errorShape("bad mode", () => locks.request("bad-mode", { mode: "invalid" } as any, () => "ok"));
  await errorShape("bad signal", () => locks.request("bad-signal", { signal: {} } as any, () => "ok"));
  await errorShape(
    "ifAvailable steal",
    () => locks.request("bad-combo", { ifAvailable: true, steal: true } as any, () => "ok"),
  );

  let releaseBusy: (value: string) => void = () => {};
  const busy = locks.request("if-available", () => {
    return new Promise((resolve) => {
      releaseBusy = resolve as (value: string) => void;
    });
  });

  const ifAvailable = await locks.request("if-available", { ifAvailable: true }, (lock: any) => {
    console.log("ifAvailable callback lock:", lock === null);
    return lock === null ? "not-available" : "available";
  });

  console.log("ifAvailable result:", ifAvailable);
  releaseBusy("busy-done");
  console.log("busy result:", await busy);
}

main().catch((err) => {
  console.log("error:", err?.name, err?.code, err?.message);
});
