import { load } from "./generated/bindings";
import type { ComponentFuture } from "perry/component";

// Static binding qualification; execution uses the canonical adapter fixtures.
export async function typedValues(bytes: Uint8Array): Promise<void> {
  const component = load(bytes, {});
  const exports = component.exports["test:async-values/values@1.0.0"];
  const future = await exports.create();
  // Awaiting the handle itself preserves it, rather than assimilating its payload.
  const preserved: typeof future = await future;
  const packet = await preserved.read();
  const sequence: bigint = packet.sequence;
  const body: Uint8Array = packet.bytes;
  packet.entry.dispose();
  // @ts-expect-error Canonical futures deliberately have no then method.
  future.then(() => {});
  // @ts-expect-error Application objects cannot satisfy the private handle brand.
  const forged: ComponentFuture<number> = { read: async () => 1, dispose: () => {} };
  const nested = await exports.nested();
  const inner = await nested.read();
  const next = await inner.read();
  const nextSequence: bigint = next.sequence;
  next.entry.dispose();
  const stream = exports.bytes();
  const chunk = await stream.read(3);
  if (!chunk.done) {
    const binary: Uint8Array = chunk.value;
    const byte: number = binary[0];
  }
  stream.dispose();
  const units = exports.units();
  const unitChunk = await units.read(4);
  if (!unitChunk.done) { const unit: void = unitChunk.value[0]; }
  units.dispose();
  const empty: void = await exports.empty().read();
  component.dispose();
}
