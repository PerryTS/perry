const { parentPort } = require("node:worker_threads");

function outcome(fn) {
  try {
    fn();
    return "ok";
  } catch (error) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const currentMask = process.umask();
parentPort.postMessage({
  chdirDisabled: process.chdir.disabled === true,
  abortDisabled: process.abort.disabled === true,
  chdir: outcome(() => process.chdir(process.cwd())),
  umask: outcome(() => process.umask(currentMask)),
});
