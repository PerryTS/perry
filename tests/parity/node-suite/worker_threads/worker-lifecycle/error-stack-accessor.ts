import { Worker } from 'node:worker_threads';

const worker = new Worker(`
  const error = new TypeError('original-error');
  Object.defineProperty(error, 'stack', {
    get() { throw new Error('stack-getter-error'); },
    configurable: true,
  });
  throw error;
`, { eval: true });
worker.on('error', (error) => {
  console.log('error:', error.name, error.message, error.stack === undefined);
});
worker.on('exit', (code) => console.log('exit:', code));
