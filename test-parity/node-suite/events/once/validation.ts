import { EventEmitter, once } from "node:events";

function errorLine(error: any): string {
  const e = error as Error & { code?: string };
  return `REJECT ${e.name} ${e.code} ${String(e.message).split("\n")[0]}`;
}

async function show(label: string, fn: () => Promise<any>): Promise<void> {
  try {
    const promise = fn();
    console.log(label, "RETURN", typeof promise?.then);
    const result = await Promise.race([
      promise.then(
        () => "FULFILLED",
        (error) => errorLine(error),
      ),
      new Promise<string>((resolve) => setTimeout(() => resolve("PENDING"), 0)),
    ]);
    console.log(label, result);
  } catch (error) {
    const e = error as Error & { code?: string };
    console.log(label, "THROW", e.name, e.code, String(e.message).split("\n")[0]);
  }
}

const emitter = new EventEmitter();

await show("number emitter", () => once(123 as any, "x"));
await show("object emitter", () => once({} as any, "x"));
await show("number options", () => once(emitter, "x", 1 as any));
await show("null options", () => once(emitter, "x", null as any));
await show("object signal", () => once(emitter, "x", { signal: {} as any }));
await show("null signal", () => once(emitter, "x", { signal: null as any }));
