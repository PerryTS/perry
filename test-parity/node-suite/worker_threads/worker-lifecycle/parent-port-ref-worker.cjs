const { parentPort } = require("node:worker_threads");

const hasRef = typeof parentPort.hasRef === "function";
const initial = hasRef ? parentPort.hasRef() : "unsupported";
const unrefReturn = typeof parentPort.unref === "function"
  ? parentPort.unref() === parentPort
  : "unsupported";
const unrefed = hasRef ? parentPort.hasRef() : "unsupported";
const refReturn = typeof parentPort.ref === "function"
  ? parentPort.ref() === parentPort
  : "unsupported";
const refed = hasRef ? parentPort.hasRef() : "unsupported";

parentPort.postMessage({ initial, unrefReturn, unrefed, refReturn, refed });
