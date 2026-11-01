import { collect } from "perry/gc";
import { asyncTypes, register } from "./generated/guest";

// The independent host supplies payload writers. These exports exercise both
// guest reads and guest writes. Async factories return readers before their
// producers finish. Independent host reads must drive those producers without
// making any more guest export calls.
let started = 0;
let held;
let completed = 0;
let failed = 0;
let chunks = 0;
function track(producer) {
  producer.then(() => { completed++; }, () => { failed++; });
}
async function writeFuture(writer, value) {
  const result = await writer.write(value);
  collect();
  if (result.written !== 1 || result.remaining.length !== 0 || !result.closed)
    throw new Error("future write did not transfer its payload");
}
async function writeStream(writer, values) {
  let remaining = values;
  while (remaining.length > 0) {
    const result = await writer.write(remaining);
    collect();
    if (result.written === 0 || result.closed || result.cancelled)
      throw new Error("stream write made no progress");
    chunks++;
    remaining = result.remaining;
  }
  writer.close();
}
async function writeCancelled(writer) {
  const pending = writer.write([17, 18, 19]);
  writer.cancel();
  const result = await pending;
  collect();
  if (!result.cancelled || result.closed || result.written !== 0 || result.remaining.length !== 3)
    throw new Error("cancel must recover the complete unconsumed batch");
  await writeStream(writer, result.remaining);
}
async function writeClosed(writer) {
  const pending = writer.write([21, 22, 23]);
  writer.close();
  const result = await pending;
  collect();
  if (!result.cancelled || !result.closed || result.written !== 0 || result.remaining.length !== 0)
    throw new Error("close must discard the complete unconsumed batch");
}
async function writeNested(outer, inner) {
  await writeFuture(outer.writer, inner.value);
  await writeFuture(inner.writer, 42);
}
register({
  "pass-future"(value) { collect(); return value; },
  async "pass-stream"(value) {
    await Promise.resolve();
    collect();
    return value;
  },
  "pass-pair"(value) { collect(); return value; },
  async character(value) { collect(); await Promise.resolve(); return value; },
  async empty(value) { collect(); await Promise.resolve(); return value; },
  async units(value) { collect(); await Promise.resolve(); return value; },
  async bytes(value) { collect(); await Promise.resolve(); return value; },
  async "nested-value"(value) { collect(); await Promise.resolve(); return value; },
  dispose(value) { value.dispose(); value.dispose(); },
  forget(_value) {},
  checkpoint() { collect(); return 73; },
  "read-started"() { collect(); return started; },
  async "read-future"(value) { started++; const result = await value.read(); collect(); return result; },
  async "read-character"(value) { started++; const result = await value.read(); collect(); return result; },
  async "read-empty"(value) { started++; return (await value.read()) === undefined; },
  async "read-values"(value) {
    started++;
    const result = [];
    for (;;) {
      const chunk = await value.read(2);
      collect();
      if (chunk.done) return result;
      for (const item of chunk.value) result.push(item);
    }
  },
  async "read-units"(value) {
    started++;
    let count = 0;
    for (;;) {
      const chunk = await value.read(2);
      collect();
      if (chunk.done) return count;
      for (const item of chunk.value) {
        if (item !== undefined) throw new Error("unit payload must be undefined");
        count++;
      }
    }
  },
  async "read-bytes"(value) {
    started++;
    const result = [];
    for (;;) {
      const chunk = await value.read(2);
      collect();
      if (chunk.done) return new Uint8Array(result);
      for (const item of chunk.value) result.push(item);
    }
  },
  async "read-nested"(value) {
    started++;
    const inner = await value.read();
    collect();
    return await inner.read();
  },
  async "dispose-pending"(value) {
    started++;
    const pending = value.read();
    value.dispose();
    try { await pending; return false; }
    catch (error) { return error.name === "AbortError"; }
  },
  async "abandon-read"(value) {
    held = value;
    value.read();
    await Promise.resolve();
    collect();
    return true;
  },
  async "read-held"() { started++; return await held.read(); },
  async "make-future"(value) {
    const pair = asyncTypes.t2_pending.create();
    track(writeFuture(pair.writer, value));
    return pair.value;
  },
  async "make-values"(value) {
    const pair = asyncTypes.t3_values.create();
    track(writeStream(pair.writer, value));
    return pair.value;
  },
  async "make-character"() {
    const pair = asyncTypes.t5_t5.create();
    track(writeFuture(pair.writer, "🍐"));
    return pair.value;
  },
  async "make-empty"() {
    const pair = asyncTypes.t6_t6.create();
    track(writeFuture(pair.writer, undefined));
    return pair.value;
  },
  async "make-units"() {
    const pair = asyncTypes.t7_t7.create();
    track(writeStream(pair.writer, [undefined, undefined, undefined, undefined, undefined]));
    return pair.value;
  },
  async "make-bytes"() {
    const pair = asyncTypes.t8_t8.create();
    track(writeStream(pair.writer, new Uint8Array([0, 1, 128, 255, 17])));
    return pair.value;
  },
  async "make-nested"() {
    const outer = asyncTypes.t10_t10.create();
    const inner = asyncTypes.t9_t9.create();
    track(writeNested(outer, inner));
    return outer.value;
  },
  async "make-cancelled"() {
    const pair = asyncTypes.t11_t11.create();
    track(writeCancelled(pair.writer));
    return pair.value;
  },
  async "make-closed"() {
    const pair = asyncTypes.t11_t11.create();
    track(writeClosed(pair.writer));
    return pair.value;
  },
  "writes-completed"() { return completed; },
  "writes-failed"() { return failed; },
  "write-chunks"() { return chunks; },
  "make-sync-future"(value) {
    const pair = asyncTypes.t2_pending.create();
    track(writeFuture(pair.writer, value));
    return pair.value;
  },
  "make-sync-values"(value) {
    const pair = asyncTypes.t3_values.create();
    track(writeStream(pair.writer, value));
    return pair.value;
  },
});
