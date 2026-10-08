import { parentPort, workerData } from 'node:worker_threads';
const gate = new Int32Array(workerData);
parentPort!.on('message', () => {
  parentPort!.postMessage(1);
  parentPort!.postMessage(2);
  parentPort!.postMessage(3);
  Atomics.store(gate, 0, 1);
  Atomics.notify(gate, 0);
});
parentPort!.postMessage('ready');
