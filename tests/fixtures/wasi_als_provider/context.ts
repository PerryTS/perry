import { AsyncLocalStorage } from "node:async_hooks";

export const storage = new AsyncLocalStorage<{ label: string }>();

export function checkStore(expected: string | undefined): void {
  const actual = storage.getStore()?.label;
  if (actual !== expected) {
    throw new Error(`provider ALS: expected ${expected}, received ${actual}`);
  }
}

export async function readAfterAwait(expected: string): Promise<void> {
  await Promise.resolve();
  checkStore(expected);
  const live = { label: expected, payload: Array.from({ length: 128 }, (_, i) => i) };
  if (typeof gc === "function") gc();
  checkStore(live.label);
  if (live.payload[127] !== 127) throw new Error("provider ALS: lost live payload");
}

declare function gc(): void;
