// Spins its event loop as fast as it can: every turn of a worker's loop used to
// drain the process's stdin buffer and call the main thread's listeners.
import { parentPort } from 'node:worker_threads';

let stopped = false;
const spin = () => {
  if (!stopped) setImmediate(spin);
};
parentPort!.on('message', () => {
  stopped = true;
  parentPort!.close();
});
setImmediate(spin);
parentPort!.postMessage('spinning');
