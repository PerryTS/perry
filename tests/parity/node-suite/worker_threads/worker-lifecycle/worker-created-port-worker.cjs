const { MessageChannel, parentPort } = require("node:worker_threads");

const { port1, port2 } = new MessageChannel();
port2.postMessage("queued-before-exit");
parentPort.postMessage({ port: port1 }, [port1]);
port2.close();
