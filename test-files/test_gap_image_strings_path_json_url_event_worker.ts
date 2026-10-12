import { Worker } from 'node:worker_threads';
const guard = setTimeout(() => process.exit(2), 30000);
const worker = new Worker(new URL('./_helpers/image_strings_worker.ts', import.meta.url));
worker.on('message', (s: string) => console.log(s));
worker.on('error', (e: any) => { console.log(e.message); process.exit(1); });
worker.on('exit', (code: number) => { clearTimeout(guard); if (code !== 0) process.exit(code); });
