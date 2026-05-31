import { pipeline, finished } from "node:stream/promises";

const delay = (ms: number) => new Promise((resolve) => setTimeout(() => resolve("pending"), ms));

async function probe(label: string, fn: () => unknown) {
  const result = await Promise.race([
    Promise.resolve()
      .then(fn)
      .then((value) => `resolved:${String(value)}`, (err: any) => `rejected:${err?.name}:${err?.code}`),
    delay(30),
  ]);
  console.log(`${label}:`, result);
}

await probe("finished number", () => finished(123 as any));
await probe("finished string", () => finished("x" as any));
await probe("pipeline no args", () => (pipeline as any)());
await probe("pipeline one number", () => (pipeline as any)(123));
await probe("pipeline number number", () => (pipeline as any)(123, 456));
