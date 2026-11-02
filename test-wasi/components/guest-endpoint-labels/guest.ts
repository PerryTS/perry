import { collect } from "perry/gc";
import { asyncTypes, register, imports } from "./generated/guest";

const foreign = imports["perry:guest-endpoint-labels/foreign@1.0.0"];
const left = imports.left;
const right = imports.right;
let completed = 0;
let rejected = 0;
let failed = 0;

function read(api, owner, expected) {
  if (api["[method]item.read"](owner.borrow()) !== expected)
    throw new Error("wrong labeled resource callback");
}
function rejectBorrow(api, owner) {
  let caught = false;
  try { api["[method]item.read"](owner.borrow()); }
  catch (_) { caught = true; }
  if (!caught) throw new Error("a label accepted another label's owner");
  rejected++;
}
function future() {
  for (const key of Object.keys(asyncTypes)) {
    if (asyncTypes[key].kind === "future") return asyncTypes[key].create();
  }
  throw new Error("missing nominal future factory");
}
async function rejectWrite(writer, owner, api, expected) {
  let caught = false;
  try { await writer.write({ owner, text: "wrong nominal resource" }); }
  catch (_) { caught = true; }
  collect();
  if (!caught) throw new Error("future accepted an owner from a different label");
  rejected++;
  // Rejection must leave the caller's ownership intact and the future writable.
  read(api, owner, expected);
}
async function produce(writer, value) {
  const a = left["[constructor]item"](value + 1000);
  const b = right["[constructor]item"](value + 2000);
  const c = foreign["[constructor]item"](value);
  collect();
  read(left, a, value + 1000);
  read(right, b, value + 2000);
  read(foreign, c, value);
  rejectBorrow(left, b);
  rejectBorrow(left, c);
  rejectBorrow(right, a);
  rejectBorrow(right, c);
  rejectBorrow(foreign, a);
  rejectBorrow(foreign, b);
  await rejectWrite(writer, a, left, value + 1000);
  await rejectWrite(writer, b, right, value + 2000);
  a.dispose();
  b.dispose();
  collect();
  const result = await writer.write({ owner: c, text: "labels 日本語 🍐" });
  collect();
  if (result.written !== 1 || result.remaining.length !== 0 || !result.closed)
    throw new Error("labeled resource future custody");
}
register({
  "perry:guest-endpoint-labels/api@1.0.0": {
    "make-future"(value) {
      const pair = future();
      produce(pair.writer, value).then(() => { completed++; }, () => {
        failed++;
        pair.writer.close();
      });
      return pair.value;
    },
    "resource-value"(value) {
      collect();
      return foreign["[method]item.read"](value);
    },
    checkpoint() { collect(); return { completed, rejected, failed }; },
  },
});
