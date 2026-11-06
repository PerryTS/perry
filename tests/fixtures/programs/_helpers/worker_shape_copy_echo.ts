import { parentPort } from 'node:worker_threads';
parentPort!.on('message', (value: any) => { parentPort!.postMessage(value); });
