const { isMainThread, parentPort } = require("node:worker_threads");

function outcome(fn) {
  try { fn(); return "ok"; }
  catch (error) { return `${error.name}:${error.code ?? ""}`; }
}

delete process.env.NODE_CHANNEL_FD;
const processValue = process.getBuiltinModule("node:process");
const workerValue = process.getBuiltinModule("node:worker_threads");
workerValue.isMainThread = true;
const capturedAbort = processValue.abort;
const title = process.title + " worker";
process.title = title;
const mask = process.umask();
const calls = {
  direct: outcome(() => process.abort()),
  captured: outcome(() => capturedAbort()),
  chdir: outcome(() => process.chdir(undefined)),
  setter: outcome(() => process.umask(mask)),
  undefinedMask: outcome(() => process.umask(undefined)),
};
const hidden = ["_startProfilerIdleNotifier", "_stopProfilerIdleNotifier", "_debugProcess", "_debugPause", "_debugEnd"];
parentPort.postMessage({
  isMainThread,
  forged: workerValue.isMainThread,
  disabled: processValue.abort.disabled,
  writableTitle: process.title === title,
  calls,
  ipcAbsent: ["send", "disconnect", "channel", "connected"].every((name) => processValue[name] === undefined && !(name in processValue)),
  hidden: hidden.every((name) => !(name in processValue) && !Object.getOwnPropertyNames(processValue).includes(name)),
});
