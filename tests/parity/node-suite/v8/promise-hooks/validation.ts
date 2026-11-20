import { promiseHooks } from "node:v8";

for (
  const [label, call] of [
    ["onInit undefined", () => promiseHooks.onInit(undefined as any)],
    ["onBefore number", () => promiseHooks.onBefore(1 as any)],
    ["onAfter async", () => promiseHooks.onAfter(async () => {})],
    [
      "onSettled generator",
      () => promiseHooks.onSettled(async function* () {}),
    ],
    ["createHook null", () => promiseHooks.createHook(null as any)],
    ["createHook bad init", () => promiseHooks.createHook({ init: 1 as any })],
  ] as const
) {
  try {
    call();
    console.log(label + ": no throw");
  } catch (error: any) {
    console.log(label + ":", error.name, error.code ?? "no-code");
  }
}
