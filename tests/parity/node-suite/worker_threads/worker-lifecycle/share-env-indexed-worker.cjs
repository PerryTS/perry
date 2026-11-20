const { parentPort } = require("node:worker_threads");

const initial = process.env[123];
process.env[456] = "from-worker";
delete process.env[7];
parentPort.postMessage({
  initial,
  keys: Object.keys(process.env).filter((key) =>
    key === "123" || key === "456"
  ),
});
