import { Worker } from 'node:worker_threads';

function observe(label: string, worker: Worker) {
  return new Promise<void>((resolve) => {
    const events: string[] = [];
    if (label === 'terminate') {
      worker.on('message', () => { worker.terminate(); });
    }
    worker.on('error', (error: any) => {
      const descriptor = Object.getOwnPropertyDescriptor(error, 'cause');
      const cause = error.cause;
      const marker = cause && typeof cause === 'object' ? cause.marker : cause;
      events.push(['error', error.name, error.message, typeof cause,
        marker === undefined ? 'undefined' : marker,
        descriptor ? descriptor.enumerable : 'absent',
        descriptor ? descriptor.configurable : 'absent',
        descriptor ? descriptor.writable : 'absent',
        error.code === undefined ? 'none' : typeof error.code === 'object' ? error.code.marker : error.code].join(':'));
    });
    worker.on('exit', (code) => {
      events.push('exit:' + code);
      console.log(label, events.join('|'));
      resolve();
    });
  });
}

for (const kind of ['object', 'undefined', 'throw', 'shadow', 'data', 'replace', 'constructor', 'terminate', 'code-mutation', 'forged']) {
  await observe(kind, new Worker(new URL('./fixtures/error-cause-properties-worker.ts', import.meta.url), {
    workerData: { kind },
  }));
}
const setup = `
  const error = new TypeError('phase');
  let reads = 0;
  Object.defineProperty(error, 'stack', { get() { reads++; return 'stack'; } });
  Object.defineProperty(error, 'cause', { get() { return reads; }, configurable: true });
`;
await observe('eval-entry', new Worker(setup + 'throw error;', { eval: true }));
await observe('eval-timer', new Worker(setup + 'setTimeout(() => { throw error; }, 0);', { eval: true }));
