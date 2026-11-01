import { collect } from "perry/gc";
import { asyncTypes, register, resources } from "./generated/guest";

const item = resources["perry:guest-endpoint-resources/api@1.0.0"].item;
let dropped = 0;
let completed = 0;
let failed = 0;
let chunks = 0;
let started = 0;
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
  const result = await writer.write(value);
  collect();
  if (result.written !== 1 || result.remaining.length !== 0 || !result.closed)
    throw new Error("resource future custody");
}
async function writeValues(writer, values) {
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
  "perry:guest-endpoint-resources/api@1.0.0": {
    "[constructor]item"(value) {
      return item.create({ value }, () => { collect(); dropped++; });
    },
    "[method]item.read"(value) { collect(); return item.value(value).value; },
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
      return result;
    },
    async "read-values"(value) {
      started++;
      const result = [];
      for (;;) {
        const chunk = await value.read(1);
        collect();
        if (chunk.done) return result;
        for (const payload of chunk.value) result.push(payload);
      }
    },
    checkpoint() { collect(); return { dropped, completed, failed, chunks, started }; },
  },
  "perry:guest-endpoint-resources/mirror@1.0.0": {
    "pass-payload"(value) { collect(); return value; },
    "pass-future"(value) { collect(); return value; },
  },
});
