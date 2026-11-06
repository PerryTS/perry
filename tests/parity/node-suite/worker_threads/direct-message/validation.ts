import { postMessageToThread } from "node:worker_threads";

async function check(label: string, fn: () => any) {
  try {
    await fn();
    console.log(label, "resolved");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

async function main() {
  await check("missing id:", () => (postMessageToThread as any)());
  await check("string id:", () => postMessageToThread("1" as any, "value"));
  await check("nan id:", () => postMessageToThread(Number.NaN, "value"));
  await check("fraction id:", () => postMessageToThread(1.5, "value"));
  await check("unknown id:", () => postMessageToThread(2147483647, "value"));
  await check(
    "bad timeout:",
    () => postMessageToThread(2147483647, "value", undefined, -1),
  );
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
