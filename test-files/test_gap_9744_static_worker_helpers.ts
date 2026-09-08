import { Worker } from 'node:worker_threads';

function identity(path: string | URL) { return path; }
const same = identity;
const workerUrl = (name: string) => new URL(`./_helpers/${name}.ts`, import.meta.url);
const entry = () => same(identity(workerUrl('static_worker_9744')));
const worker = new Worker(entry());
worker.on('message', (data: string) => {
    console.log('node', data);
    worker.terminate().then(() => process.exit(0));
});
setTimeout(() => process.exit(2), 5000);
