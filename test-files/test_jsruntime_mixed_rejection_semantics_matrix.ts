import {
  awaitRejectingCallback,
  getRejectingCallbackCatchCount,
  rejectDirect,
  rejectLate,
  resetRejectingCallbackCatchCount,
} from "./fixtures/jsruntime_promise_surface/rejections.js";

function exact(label, value, expected, count) {
  const countText = "" + count;
  if (countText === "0") {
    return label + ":missing";
  }
  if (countText !== "1") {
    return label + ":duplicate:" + countText;
  }
  if (value === undefined) {
    return label + ":undefined";
  }
  if (value !== expected) {
    return label + ":wrong:" + value;
  }
  return value;
}

let directCount = 0;
let allCount = 0;
let lateCount = 0;

const directRaw = await rejectDirect("v8-direct").catch((reason) => {
  directCount += 1;
  return "v8-catch:" + reason;
});
const direct = exact("direct", directRaw, "v8-catch:v8-direct", directCount);

resetRejectingCallbackCatchCount();
const callbackRaw = await awaitRejectingCallback(() => {
  return Promise.reject("native-callback");
});
const callbackCount = getRejectingCallbackCatchCount();
const callback = exact(
  "callback",
  callbackRaw,
  "native-callback-caught:native-callback",
  callbackCount,
);

const allRaw = await Promise.all([
  Promise.resolve("ok"),
  rejectDirect("v8-all"),
]).catch((reason) => {
  allCount += 1;
  return "all-catch:" + reason;
});
const all = exact("all", allRaw, "all-catch:v8-all", allCount);

const lateRaw = await rejectLate("v8-late").catch((reason) => {
  lateCount += 1;
  return "late:v8-late:" + reason;
});
const late = exact("late", lateRaw, "late:v8-late:v8-late", lateCount);

console.log(
  "rejections:",
  direct,
  "|",
  callback,
  "|",
  all,
  "|",
  late,
  "|",
  "counts:",
  directCount + "," + callbackCount + "," + allCount + "," + lateCount,
);
