import { parentPort, workerData } from 'node:worker_threads';

const error: any = workerData.kind === 'forged'
  ? Object.create(TypeError.prototype)
  : workerData.kind === 'constructor'
  ? new TypeError('payload', { cause: { marker: 23 } })
  : new TypeError('payload');
if (workerData.kind === 'forged') error.message = 'payload';
error.code = workerData.kind === 'code-mutation' ? { marker: 23 } : 'E_DIAG';
let reads = 0;
const getter = () => {
  reads++;
  if (workerData.kind === 'code-mutation') error.code.marker = 99;
  if (workerData.kind === 'terminate') {
    parentPort!.postMessage('entered');
    while (true) {}
  }
  if (workerData.kind === 'throw') throw new Error('getter-error');
  return workerData.kind === 'undefined' ? undefined : { marker: 23, reads };
};
if (workerData.kind === 'forged') {
  error.cause = 23;
} else if (workerData.kind === 'shadow') {
  const prototype = Object.create(Object.getPrototypeOf(error));
  Object.defineProperty(prototype, 'cause', { get: getter, configurable: true });
  Object.setPrototypeOf(error, prototype);
  Object.defineProperty(error, 'cause', { value: undefined });
} else if (workerData.kind === 'data') {
  Object.defineProperty(error, 'cause', { value: 23 });
} else if (workerData.kind === 'replace') {
  error.cause = 23;
  Object.defineProperty(error, 'stack', {
    get() {
      Object.defineProperty(error, 'cause', { value: 99, enumerable: false, writable: false });
      return 'stack';
    },
  });
} else if (workerData.kind !== 'constructor') {
  Object.defineProperty(error, 'cause', { get: getter, enumerable: true });
}
throw error;
