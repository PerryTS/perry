import { collect } from "perry/gc";
import { asyncTypes, register, imports } from "./generated/guest";

const foreign = imports["perry:guest-endpoint-imports/foreign@1.0.0"];
let completed = 0;
let failed = 0;
let chunks = 0;
let started = 0;
function check(value) {
  const expected = value.bytes[3] * 2;
  if (foreign["[method]item.read"](value.first.borrow()) !== expected)
    throw new Error("wrong imported resource identity");
  if (value.second.tag === "some" && foreign["[method]item.read"](value.second.value.borrow()) !== expected + 1)
    throw new Error("wrong optional imported resource identity");
}
function factory(kind: string) {
  for (const key of Object.keys(asyncTypes)) {
    if (asyncTypes[key].kind === kind) return asyncTypes[key];
  }
  throw new Error("missing typed endpoint factory");
}
function track(producer) {
  producer.then(() => { completed++; }, () => { failed++; });
}
async function writeFuture(writer, value) {
  check(value);
  const result = await writer.write(value);
  collect();
  if (result.written !== 1 || result.remaining.length !== 0 || !result.closed)
    throw new Error("resource future custody");
}
async function writeValues(writer, values) {
  for (const value of values) check(value);
  let remaining = values;
  while (remaining.length > 0) {
    const result = await writer.write(remaining);
    collect();
    if (result.written === 0 || result.closed || result.cancelled)
      throw new Error("resource stream progress");
    chunks++;
    remaining = result.remaining;
  }
  writer.close();
}
async function cancelValues(writer, values) {
  const pending = writer.write(values);
  writer.cancel();
  const result = await pending;
  collect();
  if (!result.cancelled || result.closed || result.written !== 0 || result.remaining.length !== values.length)
    throw new Error("resource cancellation must recover every owner");
  await writeValues(writer, result.remaining);
}
async function closeValues(writer, values) {
  const pending = writer.write(values);
  writer.close();
  const result = await pending;
  collect();
  if (!result.cancelled || !result.closed || result.written !== 0 || result.remaining.length !== 0)
    throw new Error("resource closure must discard every owner");
}
register({
  "perry:guest-endpoint-imports/api@1.0.0": {
    "make-resource"(value) { return foreign["[constructor]item"](value); },
    "resource-value"(value) { collect(); return foreign["[method]item.read"](value); },
    "make-future"(value) {
      const pair = factory("future").create();
      track(writeFuture(pair.writer, value));
      return pair.value;
    },
    "make-values"(value) {
      const pair = factory("stream").create();
      track(writeValues(pair.writer, value));
      return pair.value;
    },
    "make-cancelled"(value) {
      const pair = factory("stream").create();
      track(cancelValues(pair.writer, value));
      return pair.value;
    },
    "make-closed"(value) {
      const pair = factory("stream").create();
      track(closeValues(pair.writer, value));
      return pair.value;
    },
    async "read-future"(value) {
      started++;
      const result = await value.read();
      collect();
      check(result);
      return result;
    },
    async "read-values"(value) {
      started++;
      const result = [];
      for (;;) {
        const chunk = await value.read(1);
        collect();
        if (chunk.done) return result;
        for (const payload of chunk.value) { check(payload); result.push(payload); }
      }
    },
    checkpoint() { collect(); return { dropped: foreign.drops(), completed, failed, chunks, started }; },
  },
  "perry:guest-endpoint-imports/mirror@1.0.0": {
    "pass-future"(value) { collect(); return value; },
  },
});
