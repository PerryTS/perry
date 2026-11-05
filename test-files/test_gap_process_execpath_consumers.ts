import { fork, spawnSync } from 'node:child_process';
import { Worker } from 'node:worker_threads';
import { isAbsolute } from 'node:path';
import { realpathSync } from 'node:fs';

if (process.argv[2] === '--execpath-child') {
  if (!process.send) process.exit(1);
  process.send(isAbsolute(process.execPath)
    && process.execPath === realpathSync(process.execPath)
    && process.argv[0] === process.execPath);
  process.disconnect();
} else {
  console.log(typeof process.send === 'undefined');
  try {
    process.disconnect();
    console.log(false);
  } catch (error) {
    console.log(error instanceof TypeError);
  }
  const result = spawnSync(process.execPath, ['-e', 'console.log(true)'], { encoding: 'utf8' });
  console.log(result.status === 0 && result.stdout === 'true\n');

  const worker = new Worker(new URL('./_helpers/process_execpath_worker.ts', import.meta.url));
  worker.on('message', (ok: boolean) => console.log(ok));
  worker.on('error', () => process.exit(1));

  const child = fork(process.argv[1], ['--execpath-child'], {
    execPath: process.execPath,
    stdio: ['ignore', 'ignore', 'inherit', 'ipc'],
  });
  child.on('message', (ok: boolean) => console.log(ok));
  child.on('error', () => process.exit(1));
  child.on('exit', (code: number) => console.log(code === 0));
}
