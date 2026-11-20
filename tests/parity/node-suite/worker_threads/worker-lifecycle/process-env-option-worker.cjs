const { parentPort } = require("node:worker_threads");

const inherited = process.env.PERRY_PROCESS_ENV_OPTION;
process.env.PERRY_PROCESS_ENV_OPTION = "worker-change";
parentPort.postMessage(`${inherited}:${process.env.PERRY_PROCESS_ENV_OPTION}`);
