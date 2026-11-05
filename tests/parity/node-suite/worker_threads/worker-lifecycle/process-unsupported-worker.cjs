const { parentPort } = require("node:worker_threads");

function outcome(fn) {
  try {
    fn();
    return "ok";
  } catch (error) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const originalTitle = process.title;
process.title = `${originalTitle} worker`;
const title = process.title === `${originalTitle} worker`;

const originalDebugPort = process.debugPort;
process.debugPort = originalDebugPort + 1;
const debugPort = process.debugPort === originalDebugPort + 1;

const currentMask = process.umask();
const names = ["abort", "chdir", "send", "disconnect"];
for (
  const name of [
    "setuid",
    "seteuid",
    "setgid",
    "setegid",
    "setgroups",
    "initgroups",
  ]
) {
  if (typeof process[name] === "function") names.push(name);
}

const stubs = {};
for (const name of names) {
  stubs[name] = {
    disabled: process[name]?.disabled === true,
    call: outcome(() => process[name]()),
  };
}

const getters = {};
for (const name of ["channel", "connected"]) {
  getters[name] = outcome(() => process[name]);
}

parentPort.postMessage({
  title,
  debugPort,
  stubs,
  getters,
  umask: outcome(() => process.umask(currentMask)),
  internals: [
    "_startProfilerIdleNotifier",
    "_stopProfilerIdleNotifier",
    "_debugProcess",
    "_debugPause",
    "_debugEnd",
  ].every((name) => !(name in process)),
});
