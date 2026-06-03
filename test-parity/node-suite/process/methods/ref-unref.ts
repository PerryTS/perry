import processDefault, { ref as namedRef, unref as namedUnref } from "node:process";
import * as processNamespace from "node:process";
import { clearTimeout, setTimeout } from "node:timers";

function outcome(fn: () => unknown): string {
  try {
    const value = fn();
    return `ok:${String(value)}`;
  } catch (error) {
    const err = error as Error & { code?: string };
    return `err:${err.name}:${err.code}:${err.message}`;
  }
}

console.log("types:", typeof process.ref, process.ref.length, process.ref.name);
console.log("types unref:", typeof process.unref, process.unref.length, process.unref.name);
console.log("default identity:", processDefault === process, processNamespace.default === processDefault);
console.log("named identity:", namedRef === process.ref, namedUnref === process.unref);
console.log("keys include:", Object.keys(process).includes("ref"), Object.keys(process).includes("unref"));
console.log(
  "namespace keys include:",
  Object.keys(processNamespace).includes("ref"),
  Object.keys(processNamespace).includes("unref"),
);

console.log("noops:", outcome(() => process.ref()), outcome(() => process.unref(null as any)));
console.log("plain noops:", outcome(() => process.ref({})), outcome(() => process.unref(1 as any)));

const timeout = setTimeout(() => {}, 1000);
console.log("initial hasRef:", timeout.hasRef());
console.log("process.unref:", outcome(() => process.unref(timeout)), timeout.hasRef());
console.log("named ref:", outcome(() => namedRef(timeout)), timeout.hasRef());
console.log("default unref:", outcome(() => processDefault.unref(timeout)), timeout.hasRef());
console.log("namespace ref:", outcome(() => processNamespace.ref(timeout)), timeout.hasRef());
clearTimeout(timeout);
