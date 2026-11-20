const { parentPort, workerData: mode } = require("node:worker_threads");

process.on("exit", (code) => {
  parentPort.postMessage(`local-exit:${code}`);
  if (mode === "explicit" || mode === "natural") process.exitCode = 9;
  if (mode === "reentrant") process.exit(8);
});

if (mode === "natural") {
  process.exitCode = 7;
  parentPort.postMessage("ready");
} else if (mode === "terminate") {
  process.exitCode = 9;
  parentPort.on("message", () => {});
  parentPort.postMessage("ready");
} else if (mode === "interrupt") {
  process.exitCode = 9;
  parentPort.postMessage("ready");
  let n = 0;
  while (true) n = (n + 1) | 0;
} else {
  try {
    if (mode === "stored") {
      process.exitCode = 6;
      process.exit();
    } else if (mode === "override") {
      process.exitCode = 6;
      process.exit(0);
    } else if (mode === "reentrant") {
      process.exit(3);
    } else {
      process.exit(5);
    }
  } catch {
    parentPort.postMessage("caught");
  } finally {
    parentPort.postMessage("finally");
  }
  parentPort.postMessage("after");
}
