const { parentPort } = require("node:worker_threads");

process.stdout.write("worker-stdout\n");
process.stderr.write("worker-stderr\n");
parentPort.postMessage("written");
