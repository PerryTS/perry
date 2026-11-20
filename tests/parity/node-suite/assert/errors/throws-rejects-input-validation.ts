import assert from "node:assert";

function show(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label + ": ok");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code);
  }
}

async function showAsync(label: string, fn: () => any): Promise<void> {
  try {
    const result = fn();
    if (result && typeof result.then === "function") {
      await result;
    }
    console.log(label + ": ok");
  } catch (err: any) {
    console.log(label + ":", err?.name, err?.code);
  }
}

show("throws number", () => assert.throws(123 as any));
show("doesNotThrow number", () => assert.doesNotThrow(123 as any));
await showAsync("rejects number", () => assert.rejects(123 as any));
await showAsync("rejects fn value", () => assert.rejects(() => 42 as any));
await showAsync("doesNotReject number", () => assert.doesNotReject(123 as any));
await showAsync("doesNotReject fn value", () => assert.doesNotReject(() => 42 as any));
